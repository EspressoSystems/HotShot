extern crate proc_macro;
use std::fmt::Display;

use hotshot_testing::test_description::GeneralTestDescriptionBuilder;
use nll::nll_todo::nll_todo;
use proc_macro::TokenStream;
use proc_macro2::TokenTree;
use quote::{format_ident, quote};
use syn::parse::Result;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Attribute, DeriveInput, Expr, ExprArray, ExprPath, ExprStruct, ExprTuple, Ident, LitBool, Path,
    Token, Type, TypePath, Visibility,
};

// TODO remove
#[derive(Debug, Clone)]
enum SupportedConsensusTypes {
    ValidatingConsensus,
    SequencingConsensus,
}

/// description of a crosstest
#[derive(derive_builder::Builder, Debug, Clone)]
struct CrossTestData {
    time_types: ExprArray,
    demo_types: ExprArray,
    signature_key_types: ExprArray,
    comm_channels: ExprArray,
    storages: ExprArray,
    test_name: Ident,
    test_description: Expr,
    slow: LitBool,
}

impl CrossTestDataBuilder {
    fn is_ready(&self) -> bool {
        self.time_types.is_some()
            && self.demo_types.is_some()
            && self.signature_key_types.is_some()
            && self.comm_channels.is_some()
            && self.storages.is_some()
            && self.test_name.is_some()
            && self.test_description.is_some()
            && self.slow.is_some()
    }
}

struct TestData {
    time_type: ExprPath,
    demo_types: ExprTuple,
    signature_key_type: ExprPath,
    comm_channel: ExprPath,
    storage: ExprPath,
    // vote_type: ExprTuple,
    test_name: Ident,
    test_description: Expr,
    slow: LitBool,
}

impl TestData {
    fn new(
        time_type: ExprPath,
        demo_types: ExprTuple,
        signature_key_type: ExprPath,
        comm_channel: ExprPath,
        storage: ExprPath,
        // vote_type: ExprTuple,
        test_name: Ident,
        test_description: Expr,
        slow: LitBool,
        ) -> Self {
        Self {
            time_type,
            demo_types,
            signature_key_type,
            comm_channel,
            storage,
            // vote_type,
            test_name,
            test_description,
            slow,
        }
    }

    fn generate_test(&self) -> TokenStream {
        let TestData {
            time_type,
            demo_types,
            signature_key_type,
            test_name,
            test_description,
            slow,
            comm_channel,
            storage,
        } = self;

        let (supported_consensus_type, demo_state) = {
            let mut tuple = demo_types.elems.iter();
            let first_ele = tuple.next().expect("First element of tuple must be the consensus type.");

            let Expr::Path(expr_path) = first_ele else { panic!("Expected path expr, got {:?}", first_ele) };
            let Some(ident) = expr_path.path.get_ident() else { panic!("Expected ident, got {:?}", expr_path.path) };
            let consensus_type =
                if ident == "SequencingConsensus" {
                    SupportedConsensusTypes::SequencingConsensus
                } else if ident == "ValidatingConsensus" {
                    SupportedConsensusTypes::ValidatingConsensus
                } else {
                    panic!("Unsupported consensus type: {ident:?}")
                };

            let demo_state = tuple.next().expect("Seecond element of tuple must state type");
            (consensus_type, demo_state)
        };

        let consensus_str = format!("{supported_consensus_type:?}");

        let time_str = time_type
            .path
            .segments
            .iter()
            .fold("".to_string(), |mut acc, s| {
                acc.push_str(&s.ident.to_string().to_lowercase());
                acc
            })
        .to_lowercase();

        let demo_str =
            demo_types
            .elems
            .iter()
            .skip(1) // skip first element, which we've already processed
            .filter_map(|x| match x {
                Expr::Path(p) => Some(p),
                _ => None,
            })
        .fold("".to_string(), |mut acc, s| {
            acc.push_str(&s.path.segments.iter().fold("".to_string(), |mut acc, s| {
                acc.push_str(&s.ident.to_string().to_lowercase());
                acc
            }));
            acc
        })
        .to_lowercase();

        let signature_key_str = signature_key_type
            .path
            .segments
            .iter()
            .fold("".to_string(), |mut acc, s| {
                acc.push_str(&s.ident.to_string().to_lowercase());
                acc
            })
        .to_lowercase();

        let mod_name = format_ident!(
            "{}_{}_{}_{}",
            consensus_str,
            time_str,
            demo_str,
            signature_key_str,
        );

        let slow_attribute = if slow.value() {
            quote! { #[cfg(feature = "slow-tests")] }
        } else { quote! {} };

        let (consensus_type, leaf, vote, proposal) = match supported_consensus_type {
            SupportedConsensusTypes::SequencingConsensus => {
                let consensus_type = quote! { hotshot_types::traits::state::SequencingConsensus };
                let leaf = quote! {
                    hotshot_types::data::SequencingLeaf<TestTypes>
                };
                let vote = quote! {
                    hotshot_types::vote::DAVote<TestTypes, #leaf>
                };
                let proposal = quote! {
                    hotshot_types::data::DAProposal<TestTypes>
                };
                (consensus_type, leaf, vote, proposal)
            },
            SupportedConsensusTypes::ValidatingConsensus => {
                let consensus_type = quote! { hotshot_types::traits::state::ValidatingConsensus };
                let leaf = quote! {
                    hotshot_types::data::ValidatingLeaf<TestTypes>
                };
                let vote = quote! {
                    hotshot_types::vote::QuorumVote<TestTypes, #leaf>
                };
                let proposal = quote! {
                    hotshot_types::data::ValidatingProposal<TestTypes, #leaf>
                };
                (consensus_type, leaf, vote, proposal)
            },
        };

        quote! {

            pub mod #mod_name {
                use super::*;
                #[derive(
                    Copy,
                    Clone,
                    Debug,
                    Default,
                    Hash,
                    PartialEq,
                    Eq,
                    PartialOrd,
                    Ord,
                    serde::Serialize,
                    serde::Deserialize,
                    )]
                    pub struct Stub;

                #[derive(
                    Copy,
                    Clone,
                    Debug,
                    Default,
                    Hash,
                    PartialEq,
                    Eq,
                    PartialOrd,
                    Ord,
                    serde::Serialize,
                    serde::Deserialize,
                    )]
                    struct TestTypes;
                impl hotshot_types::traits::node_implementation::NodeType for TestTypes {
                    type ConsensusType = #consensus_type;
                    type Time = #time_type;
                    type BlockType = <#demo_state as hotshot_types::traits::State>::BlockType;
                    type SignatureKey = #signature_key_type;
                    type Transaction = <<#demo_state as hotshot_types::traits::State>::BlockType as hotshot_types::traits::Block>::Transaction;
                    type StateType = #demo_state;
                    type VoteTokenType = hotshot::traits::election::static_committee::StaticVoteToken<Self::SignatureKey>;
                    type ElectionConfigType = hotshot::traits::election::static_committee::StaticElectionConfig;
                }

                type CommitteeMembership = hotshot::traits::election::static_committee::GeneralStaticCommittee<TestTypes, #leaf, #signature_key_type>;


                #slow_attribute
                #[cfg_attr(
                    feature = "tokio-executor",
                    tokio::test(flavor = "multi_thread", worker_threads = 2)
                    )]
                    #[cfg_attr(feature = "async-std-executor", async_std::test)]
                    #[tracing::instrument]
                    async fn #test_name() {
                        let description = #test_description;

                        description.build::<
                            TestTypes, hotshot_testing::TestNodeImpl<
                            TestTypes,
                            #leaf,
                            #proposal,
                            #vote,
                            #comm_channel<
                                TestTypes,
                                #proposal,
                                #vote,
                                CommitteeMembership,
                            >,
                            #storage<TestTypes, #leaf>,
                            CommitteeMembership,
                        >>().execute().await.unwrap();

                    }
            }

        }
        .into()
    }
}

mod keywords {
    syn::custom_keyword!(Time);
    syn::custom_keyword!(DemoType);
    syn::custom_keyword!(SignatureKey);
    syn::custom_keyword!(Vote);
    syn::custom_keyword!(TestName);
    syn::custom_keyword!(TestDescription);
    syn::custom_keyword!(Slow);
    syn::custom_keyword!(CommChannel);
    syn::custom_keyword!(Storage);
}

impl Parse for CrossTestData {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut description = CrossTestDataBuilder::create_empty();

        while !description.is_ready() {
            if input.peek(keywords::Time) {
                let _ = input.parse::<keywords::Time>()?;
                input.parse::<Token![:]>()?;
                let time_types = input.parse::<ExprArray>()?;
                description.time_types(time_types);
            }
            else if input.peek(keywords::DemoType) {
                let _ = input.parse::<keywords::DemoType>()?;
                input.parse::<Token![:]>()?;
                let demo_types = input.parse::<ExprArray>()?;
                description.demo_types(demo_types);
            } else if input.peek(keywords::SignatureKey) {
                let _ = input.parse::<keywords::SignatureKey>()?;
                input.parse::<Token![:]>()?;
                let signature_key_types = input.parse::<ExprArray>()?;
                description.signature_key_types(signature_key_types);
            } else if input.peek(keywords::CommChannel) {
                let _ = input.parse::<keywords::CommChannel>()?;
                input.parse::<Token![:]>()?;
                let comm_channels = input.parse::<ExprArray>()?;
                description.comm_channels(comm_channels);
            } else if input.peek(keywords::Storage) {
                let _ = input.parse::<keywords::Storage>()?;
                input.parse::<Token![:]>()?;
                let storages = input.parse::<ExprArray>()?;
                description.storages(storages);
            } else if input.peek(keywords::TestName) {
                let _ = input.parse::<keywords::TestName>()?;
                input.parse::<Token![:]>()?;
                let test_name = input.parse::<Ident>()?;
                description.test_name(test_name);
            } else if input.peek(keywords::TestDescription) {
                let _ = input.parse::<keywords::TestDescription>()?;
                input.parse::<Token![:]>()?;
                let test_description = input.parse::<Expr>()?;
                description.test_description(test_description);
            } else if input.peek(keywords::Slow) {
                let _ = input.parse::<keywords::Slow>()?;
                input.parse::<Token![:]>()?;
                let slow = input.parse::<LitBool>()?;
                description.slow(slow);
            } else {
                panic!("Unexpected token. Expected one f: Time, DemoType, SignatureKey, CommChannel, Storage, TestName, TestDescription, Slow");
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        description.build().map_err(|e| syn::Error::new(proc_macro2::Span::call_site(), format!("{}", e)))
    }
}

#[proc_macro]
pub fn cross_tests(input: TokenStream) -> TokenStream {
    let test_spec = parse_macro_input!(input as CrossTestData);

    let mut tokens = TokenStream::new();

    let demo_types = test_spec
        .demo_types
        .elems
        .iter()
        .filter_map(|t| {
            let Expr::Tuple(p) = t else { panic!("Expected Tuple! Got {:?}", t) };
            Some(p)
        }).collect::<Vec<_>>();

    let comm_channels = test_spec.comm_channels.elems.iter()
        .filter_map(|t| {
            let Expr::Path(p) = t else { panic!("Expected Path for Comm Channel! Got {:?}", t) };
            Some(p)
        });

    let storages = test_spec.storages.elems.iter().filter_map(|t|{
        let Expr::Path(p) = t else { panic!("Expected Path for Storage! Got {:?}", t) };
        Some(p)
    });

    let time_types = test_spec.time_types.elems.iter().filter_map(|t|{
        let Expr::Path(p) = t else { panic!("Expected Path for Time Type! Got {:?}", t) };
        Some(p)
    });

    let signature_key_types = test_spec.signature_key_types.elems.iter().filter_map(|t|{
        let Expr::Path(p) = t else { panic!("Expected Path for Signature Key Type! Got {:?}", t) };
        Some(p)
    });

    for demo_type in demo_types.clone() {
        for comm_channel in comm_channels.clone() {
            for storage in storages.clone() {
                for time_type in time_types.clone() {
                    for signature_key_type in signature_key_types.clone() {
                        let test_data = TestData::new(
                            time_type.clone(),
                            demo_type.clone().clone(),
                            signature_key_type.clone(),
                            comm_channel.clone(),
                            storage.clone(),
                            test_spec.test_name.clone(),
                            test_spec.test_description.clone(),
                            test_spec.slow.clone(),
                            );
                        let test = test_data.generate_test();
                        tokens.extend(test);
                    }
                }
            }
        }
    }
    tokens
}

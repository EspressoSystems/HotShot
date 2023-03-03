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
    consensus_type: SupportedConsensusTypes,
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
        consensus_type: SupportedConsensusTypes,
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
            consensus_type,
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
            consensus_type,
            time_type,
            demo_types,
            signature_key_type,
            // vote_type,
            test_name,
            test_description,
            slow,
            comm_channel,
            storage,
        } = self;

        let consensus_str = format!("{consensus_type:?}");

        let time_str = time_type
            .path
            .segments
            .iter()
            .fold("".to_string(), |mut acc, s| {
                acc.push_str(&s.ident.to_string().to_lowercase());
                acc
            })
        .to_lowercase();

        let demo_str = demo_types
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

        let demo_state = demo_types.elems.iter().skip(1).next().unwrap();

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

        let (consensus_type, leaf, vote, proposal) = match consensus_type {
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
                        >
                            >().execute().await.unwrap();

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
    // TODO make order not matter.
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
    // panic!("{:#?}", test_spec.demo_types);

    let mut tokens = TokenStream::new();

    let consensus_types = test_spec.demo_types.elems.iter().filter_map(
        |expr| {
            if let Expr::Tuple(tuple) = expr {

                let first_ele = tuple.elems.iter().next().expect("First element of array must be the consensus type.");
                if let Expr::Path(expr_path) = first_ele {
                    if let Some(ident) = expr_path.path.get_ident() {
                        return if ident == "SequencingConsensus" {
                            Some(SupportedConsensusTypes::SequencingConsensus)
                        } else if ident == "ValidatingConsensus" {
                            Some(SupportedConsensusTypes::ValidatingConsensus)
                        } else {
                            panic!("Unsupported consensus type: {ident:?}")
                        }
                    } else {
                        return None;
                    }
                }
                return None;
            };
            return None;
        }
        ).collect::<Vec<_>>();

    let demo_types = test_spec
        .demo_types
        .elems
        .iter()
        .filter_map(|t| match t {
            Expr::Tuple(p) => Some(p),
            _ => None,
        }).collect::<Vec<_>>();

    if demo_types.len() != consensus_types.len() {
        panic!("Demo types length did not match consensus types length!");
    }

    let comm_channels = test_spec.comm_channels.elems.iter().filter_map(|t| match t {
        Expr::Path(p) => Some(p),
        _ => None,
    });

    let storages = test_spec.storages.elems.iter().filter_map(|t| match t {
        Expr::Path(p) => Some(p),
        _ => None,
    });

    // TODO better error handling instead of `None`. We want to fail loudly


    for (consensus_type, demo_type) in consensus_types.into_iter().zip(demo_types.iter()) {
        for comm_channel in comm_channels.clone() {
            for storage in storages.clone() {
                for time_type in test_spec.time_types.elems.iter().filter_map(|t| match t {
                    Expr::Path(p) => Some(p),
                        _ => None,
                }) {
                    for signature_key_type in
                        test_spec
                            .signature_key_types
                            .elems
                            .iter()
                            .filter_map(|t| match t {
                                Expr::Path(p) => Some(p),
                                _ => None,
                            })
                    {
                        let comm_channel = comm_channel.clone();
                        let test_data = TestData::new(
                            consensus_type.clone(),
                            time_type.clone(),
                            demo_type.clone().clone(),
                            signature_key_type.clone(),
                            comm_channel,
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

    // panic!("{:#?}", tokens.to_string());
}

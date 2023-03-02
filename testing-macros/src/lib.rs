extern crate proc_macro;
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

/// Parses the following syntax, which aligns with the input of the real
/// `lazy_static` crate.
///
///     lazy_static! {
///         $VISIBILITY static ref $NAME: $TYPE = $EXPR;
///     }
///
/// For example:
///
///     lazy_static! {
///         static ref USERNAME: Regex = Regex::new("^[a-z0-9_-]{3,16}$").unwrap();
///     }
struct CrossTestData {
    time_types: ExprArray,
    demo_types: ExprArray,
    signature_key_types: ExprArray,
    // vote_types: ExprArray,
    comm_channels: ExprArray,
    storages: ExprArray,
    test_name: Ident,
    test_description: Expr,
    slow: LitBool,
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

        let (demo_block, demo_state, demo_transaction) = {
            let mut demos = demo_types.elems.iter().skip(1); // first ele in consensustype
            let d1 = demos.next().unwrap();
            let d2 = demos.next().unwrap();
            let d3 = demos.next().unwrap();
            (d1, d2, d3)
        };

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
                    type BlockType = #demo_block;
                    type SignatureKey = #signature_key_type;
                    type Transaction = #demo_transaction;
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
        let _ = input.parse::<keywords::Time>()?;
        input.parse::<Token![:]>()?;
        let time_types = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::DemoType>()?;
        input.parse::<Token![:]>()?;
        let demo_types = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::SignatureKey>()?;
        input.parse::<Token![:]>()?;
        let signature_key_types = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::CommChannel>()?;
        input.parse::<Token![:]>()?;
        let comm_channels = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::Storage>()?;
        input.parse::<Token![:]>()?;
        let storages = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::TestName>()?;
        input.parse::<Token![:]>()?;
        let test_name = input.parse::<Ident>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::TestDescription>()?;
        input.parse::<Token![:]>()?;
        let test_description = input.parse::<Expr>()?;
        input.parse::<Token![,]>()?;

        let _ = input.parse::<keywords::Slow>()?;
        input.parse::<Token![:]>()?;
        let slow = input.parse::<LitBool>()?;
        // consume the rest of the input stream, it's ok if this fails
        let _ = input.parse::<Token![,]>();

        // check_ty_match(r.clone(), "ConsensusType")?;
        // let types = input.parse::<Punctuated<Type, Token![,]>>()?;

        // let r = format!("{:#?}", consensus_types);
        // let cursor = input.cursor();
        // let r = format!("{:?}", cursor.literal().map(|x| x.0));
        // eprintln!("{:?}", r);
        // for i in 0..3 {
        // let p = input.peek(Token![:]);
        // input.pee
        // }
        // while !(input.peek(Token![:]) && !input.peek(Token![::])) {
        //     let tokens = input.parse::<TokenTree>();
        //     eprintln!("{:#?}", tokens);
        // }

        Ok(CrossTestData {
            comm_channels,
            storages,
            time_types,
            demo_types,
            signature_key_types,
            // vote_types,
            test_name,
            test_description,
            slow,
        })

        // Err(syn::Error::new(proc_macro2::Span::call_site(), r))
        // Err(syn::Error::new(proc_macro2::Span::call_site(), "We're not done yet!"))

        // let ty = input.parse::<Type>()?;
        // input.parse::<Token![:]>()?;
        // let name: Ident = input.parse()?;
        // input.parse::<Token![:]>()?;
        // let ty: Type = input.parse()?;
        // input.parse::<Token![=]>()?;
        // let init: Expr = input.parse()?;
        // input.parse::<Token![;]>()?;
        // Ok(LazyStatic {
        //     visibility,
        //     name,
        //     ty,
        //     init,
        // })
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
                            // vote_type.clone(),
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

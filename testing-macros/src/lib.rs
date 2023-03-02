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
    consensus_types: ExprArray,
    time_types: ExprArray,
    demo_types: ExprArray,
    signature_key_types: ExprArray,
    vote_types: ExprArray,
    test_name: Ident,
    test_description: Expr,
    slow: LitBool,
}

struct TestData {
    consensus_type: ExprPath,
    time_type: ExprPath,
    demo_types: ExprTuple,
    signature_key_type: ExprPath,
    vote_type: ExprTuple,
    test_name: Ident,
    test_description: Expr,
    slow: LitBool,
}

impl TestData {
    fn new(
        consensus_type: ExprPath,
        time_type: ExprPath,
        demo_types: ExprTuple,
        signature_key_type: ExprPath,
        vote_type: ExprTuple,
        test_name: Ident,
        test_description: Expr,
        slow: LitBool,
    ) -> Self {
        Self {
            consensus_type,
            time_type,
            demo_types,
            signature_key_type,
            vote_type,
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
            vote_type,
            test_name,
            test_description,
            slow,
        } = self;

        let consensus_str = consensus_type
            .path
            .segments
            .iter()
            .fold("".to_string(), |mut acc, s| {
                acc.push_str(&s.ident.to_string().to_lowercase());
                acc
            })
            .to_lowercase();

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
            let mut demos = demo_types.elems.iter();
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

        let vote_type_str = vote_type
            .elems
            .iter()
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

        let (vote_token, vote_config) = {
            let mut demos = vote_type.elems.iter();
            let d1 = demos.next().unwrap();
            let d2 = demos.next().unwrap();
            (d1, d2)
        };

        let test_name_str = test_name.to_string().to_lowercase();

        let mod_name = format_ident!(
            "{}_{}_{}_{}_{}",
            consensus_str,
            time_str,
            demo_str,
            signature_key_str,
            vote_type_str
        );

        //     .path.segments.iter().fold("".to_string(), |mut acc, s| {
        //     acc.push_str(&s.ident.to_string().to_lowercase());
        //     acc
        // });

        quote! {

            pub mod #mod_name {
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
                impl hotshot_types::traits::node_implementation::ApplicationMetadata for Stub {}

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
                    type VoteTokenType = #vote_token<#signature_key_type>;
                    // valid assumption that it takes in a signature key? not sure.
                    type ElectionConfigType = #vote_config;

                    // we're nuking this lol
                    type ApplicationMetadataType = Stub;

                }


                #[test]
                // #slow
                fn #test_name() {
                    // let test_type = hotshot_testing::TestDescription<
                    //     TestTypes,
                    //     hotshot_testing::TestNodeImpl<
                    //         #
                    //     >
                    // >;
                }
            }

        }
        .into()
    }
}

mod keywords {
    syn::custom_keyword!(ConsensusType);
    syn::custom_keyword!(Time);
    syn::custom_keyword!(DemoType);
    syn::custom_keyword!(SignatureKey);
    syn::custom_keyword!(Vote);
    syn::custom_keyword!(TestName);
    syn::custom_keyword!(TestDescription);
    syn::custom_keyword!(Slow);
}

impl Parse for CrossTestData {
    fn parse(input: ParseStream) -> Result<Self> {
        let _ = input.parse::<keywords::ConsensusType>()?;
        input.parse::<Token![:]>()?;
        let consensus_types = input.parse::<ExprArray>()?;
        input.parse::<Token![,]>()?;

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

        let _ = input.parse::<keywords::Vote>()?;
        input.parse::<Token![:]>()?;
        let vote_types = input.parse::<ExprArray>()?;
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
            consensus_types,
            time_types,
            demo_types,
            signature_key_types,
            vote_types,
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

    for consensus_type in test_spec
        .consensus_types
        .elems
        .iter()
        .filter_map(|t| match t {
            Expr::Path(p) => Some(p),
            _ => None,
        })
    {
        for time_type in test_spec.time_types.elems.iter().filter_map(|t| match t {
            Expr::Path(p) => Some(p),
            _ => None,
        }) {
            for demo_type in test_spec
                .demo_types
                .clone()
                .elems
                .iter()
                .filter_map(|t| match t {
                    Expr::Tuple(p) => Some(p),
                    _ => None,
                })
            {
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
                    for vote_type in test_spec.vote_types.elems.iter().filter_map(|t| match t {
                        Expr::Tuple(p) => Some(p),
                        _ => None,
                    }) {
                        let test_data = TestData::new(
                            consensus_type.clone(),
                            time_type.clone(),
                            demo_type.clone(),
                            signature_key_type.clone(),
                            vote_type.clone(),
                            test_spec.test_name.clone(),
                            test_spec.test_description.clone(),
                            test_spec.slow.clone(),
                        );
                        let test = test_data.generate_test();

                        // let test_name = format!("{}_{}_{}_{}_{}", test_spec.test_name, consensus_type, time_type, demo_type, signature_key_type, vote_type);
                        // let test_description = format!("{} {} {} {} {}", test_spec.test_description, consensus_type, time_type, demo_type, signature_key_type, vote_type);
                        // let test = quote! {
                        //   #[test]
                        //   #[cfg_attr(not(feature = "slow_tests"), ignore)]
                        //   fn #test_name() {
                        //     let consensus_type = #consensus_type;
                        //     let time_type = #time_type;
                        //     let demo_type = #demo_type;
                        //     let signature_key_type = #signature_key_type;
                        //     let vote_type = #vote_type;
                        //     let test_description = #test_description;
                        //     let slow = #test_spec.slow;
                        //     run_test(consensus_type, time_type, demo_type, signature_key_type, vote_type, test_description, slow);
                        //   }
                        // };
                        tokens.extend(test);
                    }
                }
            }
        }
    }
    tokens

    // panic!("{:#?}", tokens.to_string());
}

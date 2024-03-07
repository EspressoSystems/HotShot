//! Macros for testing over all network implementations and nodetype implementations

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::Result;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, ExprArray, ExprPath, ExprTuple, Ident, LitBool, Token,
};

#[proc_macro]
pub fn test_scripts(input: proc_macro::TokenStream) -> TokenStream {
    // Parse the input as an iter of Expr
    let inputs: Vec<_> = syn::parse::Parser::parse2(
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        input.into(),
    )
    .unwrap()
    .into_iter()
    .collect();

    let test_inputs = &inputs[0];

    let scripts = &inputs[1..];

    let output_index_names: Vec<_> = scripts
        .clone()
        .into_iter()
        .map(|i| format_ident!("{}_output_index", quote::quote!(#i).to_string())).collect();

    let task_names: Vec<_> = scripts
        .clone()
        .into_iter()
        .map(|i| format_ident!("{}_task", quote::quote!(#i).to_string())).collect();

    let task_expectations: Vec<_> = scripts
        .clone()
        .into_iter()
        .map(|i| format_ident!("{}_expectations", quote::quote!(#i).to_string())).collect();

    let expanded = quote! {

    use hotshot_testing::predicates::Predicate;
    use async_broadcast::broadcast;
    use hotshot_task_impls::events::HotShotEvent;

    use hotshot_task::task::{Task, TaskRegistry, TaskState};
    use hotshot_types::traits::node_implementation::NodeType;
    use std::sync::Arc;

    let registry = Arc::new(TaskRegistry::default());

    let (test_input, task_receiver) = broadcast(1024);
    // let (task_input, mut test_receiver) = broadcast(1024);

    let task_input = test_input.clone();
    let mut test_receiver = task_receiver.clone();

    let mut loop_receiver = task_receiver.clone();

    #(let mut #task_names = Task::new(task_input.clone(), task_receiver.clone(), registry.clone(), #scripts.state);)*

    #(let mut #task_expectations = #scripts.expectations;)*

    #(let mut #output_index_names = 0;)*

    for (stage_number, input_group) in #test_inputs.into_iter().enumerate() {
        for input in &input_group {
            #(
                if !#task_names.state().filter(input) {
                    tracing::debug!("Test sent: {:?}", input);

                    if let Some(res) = #task_names.handle_event(input.clone()).await {
                        #task_names.state().handle_result(&res).await;
                    }

                    while let Ok(received_output) = test_receiver.try_recv() {
                        tracing::debug!("Test received: {:?}", received_output);

                        let output_asserts = &#task_expectations[stage_number].output_asserts;

                        if #output_index_names >= output_asserts.len() {
                            panic_extra_output(stage_number, &received_output);
                        };

                        let assert = &output_asserts[#output_index_names];

                        validate_output_or_panic(stage_number, &received_output, assert);

                        #output_index_names += 1;
                    }
                }
            )*
        }

        while let Ok(input) = loop_receiver.try_recv() {
            #(
                if !#task_names.state().filter(&input) {
                    tracing::debug!("Test sent: {:?}", input);

                    if let Some(res) = #task_names.handle_event(input.clone()).await {
                        #task_names.state().handle_result(&res).await;
                    }

                    while let Ok(received_output) = test_receiver.try_recv() {
                        tracing::debug!("Test received: {:?}", received_output);

                        let output_asserts = &#task_expectations[stage_number].output_asserts;

                        if #output_index_names >= output_asserts.len() {
                            panic_extra_output(stage_number, &received_output);
                        };

                        let assert = &output_asserts[#output_index_names];

                        validate_output_or_panic(stage_number, &received_output, assert);

                        #output_index_names += 1;
                    }
                }
            )*
        }

        #(
            let output_asserts = &#task_expectations[stage_number].output_asserts;

            if #output_index_names < output_asserts.len() {
                panic_missing_output(stage_number, &output_asserts[#output_index_names]);
            }

            let task_state_asserts = &#task_expectations[stage_number].task_state_asserts;

            for assert in task_state_asserts {
                validate_task_state_or_panic(stage_number, #task_names.state(), assert);
            }
        )*
    }

    };
    println!("{expanded}");

    expanded.into()
}

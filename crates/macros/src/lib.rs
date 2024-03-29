//! Macros for use in testing.

use proc_macro::TokenStream;
use quote::{format_ident, quote};

/// Macro to test multiple `TaskState` scripts at once.
///
/// Usage:
///
///   `test_scripts[inputs, script1, script2, ...]`
///
/// The generated test will:
///   - take the first entry of `inputs`, which should be a `Vec<HotShotEvent<TYPES>>`,
///   - feed this to each task in order, validating any output received before moving on to the next one,
///   - repeat the previous steps, with the aggregated outputs received from all tasks used as the new input set,
///   - repeat until no more output has been generated by any task, and finally
///   - proceed to the next entry of inputs.
///
/// # Panics
///
/// The macro panics if the input stream cannot be parsed.
/// The test will panic if the any of the scripts has a different number of stages from the input.
#[allow(clippy::too_many_lines)]
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

    let test_inputs_name = quote::quote!(#test_inputs).to_string();

    let scripts = &inputs[1..];

    let output_index_names: Vec<_> = scripts
        .iter()
        .map(|i| format_ident!("{}_output_index", quote::quote!(#i).to_string()))
        .collect();

    let task_names: Vec<_> = scripts
        .iter()
        .map(|i| format_ident!("{}_task", quote::quote!(#i).to_string()))
        .collect();

    let task_expectations: Vec<_> = scripts
        .iter()
        .map(|i| format_ident!("{}_expectations", quote::quote!(#i).to_string()))
        .collect();

    let script_names: Vec<_> = scripts
        .iter()
        .map(|i| quote::quote!(#i).to_string())
        .collect();

    let expanded = quote! { {

    use hotshot_testing::script::{
        panic_extra_output_in_script, panic_missing_output_in_script, validate_output_or_panic_in_script,
        validate_task_state_or_panic_in_script,RECV_TIMEOUT_MILLIS
    };

    use hotshot_testing::predicates::Predicate;
    use async_broadcast::broadcast;
    use hotshot_task_impls::events::HotShotEvent;
    use std::time::Duration;
    use async_compatibility_layer::art::async_timeout;
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

    #(assert!(#task_expectations.len() == #test_inputs.len(), "Number of stages in {} does not match the number of stages in {}", #script_names, #test_inputs_name);)*

    for (stage_number, input_group) in #test_inputs.into_iter().enumerate() {

    #(let mut #output_index_names = 0;)*

        for input in &input_group {
            #(
                if !#task_names.state().filter(&input.clone().into()) {
                    tracing::debug!("Test sent: {:?}", input);

                    if let Some(res) = #task_names.handle_event(input.clone().into()).await {
                        #task_names.state().handle_result(&res).await;
                    }

                    while let Ok(received_output_0) = test_receiver.try_recv() {
                        tracing::debug!("Test received: {:?}", received_output_0);

                        let output_asserts = &#task_expectations[stage_number].output_asserts;

                        if #output_index_names >= output_asserts.len() {
                            panic_extra_output_in_script(stage_number, #script_names.to_string(), &received_output_0);
                        };

                        let assert = &output_asserts[#output_index_names];

                        match assert {
                            EventPredicate::One(assert) => validate_output_or_panic_in_script(stage_number, #script_names.to_string(), &received_output_0, assert),
                            EventPredicate::Consecutive(assert) => {
                                if let Ok(received_output_1) = test_receiver.try_recv() {
                                    tracing::debug!("Test received: {:?}", received_output_1);

                                    let output_asserts = &#task_expectations[stage_number].output_asserts;

                                    if #output_index_names >= output_asserts.len() {
                                        panic_extra_output_in_script(stage_number, #script_names.to_string(), &received_output_1);
                                    };

                                    validate_output_or_panic_in_script(stage_number, #script_names.to_string(), &(received_output_0, received_output_1), assert);
                                }
                            }
                        }

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

                    while let Ok(Ok(received_output_0)) = async_timeout(Duration::from_millis(RECV_TIMEOUT_MILLIS), test_receiver.recv_direct()).await {
                        tracing::debug!("Test received: {:?}", received_output_0);

                        let output_asserts = &#task_expectations[stage_number].output_asserts;

                        if #output_index_names >= output_asserts.len() {
                            panic_extra_output_in_script(stage_number, #script_names.to_string(), &received_output_0);
                        };

                        let assert = &output_asserts[#output_index_names];

                        match assert {
                            EventPredicate::One(assert) => validate_output_or_panic_in_script(stage_number, #script_names.to_string(), &received_output_0, assert),
                            EventPredicate::Consecutive(assert) => {
                                if let Ok(Ok(received_output_1)) = async_timeout(Duration::from_millis(RECV_TIMEOUT_MILLIS), test_receiver.recv_direct()).await {
                                    tracing::debug!("Test received: {:?}", received_output_1);

                                    let output_asserts = &#task_expectations[stage_number].output_asserts;

                                    if #output_index_names >= output_asserts.len() {
                                        panic_extra_output_in_script(stage_number, #script_names.to_string(), &received_output_1);
                                    };

                                    validate_output_or_panic_in_script(stage_number, #script_names.to_string(), &(received_output_0, received_output_1), assert);
                                }
                            }
                        }

                        #output_index_names += 1;
                    }
                }
            )*
        }

        #(
            let output_asserts = &#task_expectations[stage_number].output_asserts;

            if #output_index_names < output_asserts.len() {
                panic_missing_output_in_script(stage_number, #script_names.to_string(), &output_asserts[#output_index_names]);
            }

            let task_state_asserts = &#task_expectations[stage_number].task_state_asserts;

            for assert in task_state_asserts {
                validate_task_state_or_panic_in_script(stage_number, #script_names.to_string(), #task_names.state(), assert);
            }
        )*
    } }

    };

    expanded.into()
}

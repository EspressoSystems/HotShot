// /// Generate the inside of a test.
// /// Only for internal usage.
// #[macro_export]
// macro_rules! gen_inner_fn {
//     ($TEST_TYPE:ty, $e:expr) => {
//         // NOTE we need this since proptest doesn't implement async things
//         async_compatibility_layer::art::async_block_on(async move {
//             async_compatibility_layer::logging::setup_logging();
//             async_compatibility_layer::logging::setup_backtrace();
//             let description = $e;
//             let built: $TEST_TYPE = description.build();
//             built.execute().await.unwrap()
//         });
//     };
// }
//
// /// special casing for proptest. Need the inner thing to block
// #[macro_export]
// macro_rules! gen_inner_fn_proptest {
//     ($TEST_TYPE:ty, $e:expr) => {
//         // NOTE we need this since proptest doesn't implement async things
//         async_compatibility_layer::art::async_block_on_with_runtime(async move {
//             async_compatibility_layer::logging::setup_logging();
//             async_compatibility_layer::logging::setup_backtrace();
//             let description = $e;
//             let built: $TEST_TYPE = description.build();
//             built.execute().await.unwrap()
//         });
//     };
// }
//
// /// Generate a test.
// /// Args:
// /// - $TEST_TYPE: TestDescription type
// /// - $fn_name: name of test
// /// - $e: The test description
// /// - $keep: whether or not to ignore the test
// /// - $args: list of arguments to fuzz over (fed to proptest)
// #[macro_export]
// macro_rules! cross_test {
//     // base case
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: false, args: $($args:tt)+) => {
//         proptest::prelude::proptest!{
//             #![proptest_config(
//                 proptest::prelude::ProptestConfig {
//                     timeout: 300000,
//                     cases: 10,
//                     .. proptest::prelude::ProptestConfig::default()
//                 }
//                 )]
//                 #[test]
//                 fn $fn_name($($args)+) {
//                     gen_inner_fn_proptest!($TEST_TYPE, $e);
//                 }
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: true, args: $($args:tt)+) => {
//         proptest::prelude::proptest!{
//             #![proptest_config(
//                 proptest::prelude::ProptestConfig {
//                     timeout: 300000,
//                     cases: 10,
//                     .. proptest::prelude::ProptestConfig::default()
//                 }
//                 )]
//                 #[cfg(feature = "slow-tests")]
//                 #[test]
//                 fn $fn_name($($args)+) {
//                     gen_inner_fn_proptest!($TEST_TYPE, $e);
//                 }
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: false, args: $($args:tt)+) => {
//         proptest::prelude::proptest!{
//             #![proptest_config(
//                 proptest::prelude::ProptestConfig {
//                     timeout: 300000,
//                     cases: 10,
//                     .. proptest::prelude::ProptestConfig::default()
//                 }
//                 )]
//                 #[test]
//                 #[ignore]
//                 fn $fn_name($($args)+) {
//                     gen_inner_fn_proptest!($TEST_TYPE, $e);
//                 }
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: true, args: $($args:tt)+) => {
//         proptest::prelude::proptest!{
//             #![proptest_config(
//                 proptest::prelude::ProptestConfig {
//                     timeout: 300000,
//                     cases: 10,
//                     .. proptest::prelude::ProptestConfig::default()
//                 }
//                 )]
//                 #[cfg(feature = "slow-tests")]
//                 #[test]
//                 #[ignore]
//                 fn $fn_name($($args)+) {
//                     gen_inner_fn!($TEST_TYPE, $e);
//                 }
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: false, args: ) => {
//         #[cfg_attr(
//             feature = "tokio-executor",
//             tokio::test(flavor = "multi_thread", worker_threads = 2)
//         )]
//         #[cfg_attr(feature = "async-std-executor", async_std::test)]
//         async fn $fn_name() {
//             gen_inner_fn!($TEST_TYPE, $e);
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: true, args: ) => {
//         #[cfg(feature = "slow-tests")]
//         #[cfg_attr(
//             feature = "tokio-executor",
//             tokio::test(flavor = "multi_thread", worker_threads = 2)
//         )]
//         #[cfg_attr(feature = "async-std-executor", async_std::test)]
//         async fn $fn_name() {
//             gen_inner_fn!($TEST_TYPE, $e);
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: false, args: ) => {
//         #[cfg_attr(
//             feature = "tokio-executor",
//             tokio::test(flavor = "multi_thread", worker_threads = 2)
//         )]
//         #[cfg_attr(feature = "async-std-executor", async_std::test)]
//         #[ignore]
//         async fn $fn_name() {
//             gen_inner_fn!($TEST_TYPE, $e);
//         }
//     };
//     ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: true, args: ) => {
//         #[cfg(feature = "slow-tests")]
//         #[cfg_attr(
//             feature = "tokio-executor",
//             tokio::test(flavor = "multi_thread", worker_threads = 2)
//         )]
//         #[ignore]
//         async fn $fn_name() {
//             gen_inner_fn!($TEST_TYPE, $e);
//         }
//     };
// }
//
// /// Macro to generate tests for all types based on a description
// /// Arguments:
// /// - $NETWORKS: a space delimited list of Network implementations
// /// - $STORAGES: a space delimited list of Storage implementations
// /// - $BLOCKS: a space delimited list of Block implementations
// /// - $STATES: a space delimited list of State implementations
// /// - $fn_name: a identifier for the outermost test module
// /// - $expr: a TestDescription for the test
// /// - $keep:
// ///   - true is a noop
// ///   - false forces test to be ignored
// /// - $args: list of arguments to fuzz over (fed to proptest)
// ///
// // TestNodeImpl<DEntryState, MemoryStorage<DEntryState>, TestNetwork, Ed25519Pub, StaticCommittee<DEntryState>>
// #[macro_export]
// macro_rules! cross_tests {
//     // reduce networks -> individual network modules
//     ([ $NETWORK:tt $($NETWORKS:tt)* ], [ $($STORAGES:tt)+ ], [ $($BLOCKS:tt)+ ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//         #[ macro_use ]
//         #[ allow(non_snake_case) ]
//         mod $NETWORK {
//             use $crate::*;
//             cross_tests!($NETWORK, [ $($STORAGES)+ ], [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//         }
//         cross_tests!([ $($NETWORKS)*  ], [ $($STORAGES)+ ], [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)* );
//     };
//     // catchall for empty network list (base case)
//     ([  ], [ $($STORAGE:tt)+ ], [ $($BLOCKS:tt)+ ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//     };
//     // reduce storages -> individual storage modules
//     ($NETWORK:tt, [ $STORAGE:tt $($STORAGES:tt)* ], [ $($BLOCKS:tt)+ ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//         #[ macro_use ]
//         #[ allow(non_snake_case) ]
//         mod $STORAGE {
//             use $crate::*;
//             cross_tests!($NETWORK, $STORAGE, [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//         }
//         cross_tests!($NETWORK, [ $($STORAGES),* ], [ $($BLOCKS),+ ], [ $($STATES),+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//     };
//     // catchall for empty storage list (base case)
//     ($NETWORK:tt, [  ], [ $($BLOCKS:tt)+ ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//     };
//     // reduce blocks -> individual block modules
//     ($NETWORK:tt, $STORAGE:tt, [ $BLOCK:tt $($BLOCKS:tt)* ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//         #[ macro_use ]
//         #[ allow(non_snake_case) ]
//         mod $BLOCK {
//             use $crate::*;
//             cross_tests!($NETWORK, $STORAGE, $BLOCK, [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//         }
//         cross_tests!($NETWORK, $STORAGE, [ $($BLOCKS),* ], [ $($STATES),+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//     };
//     // catchall for empty block list (base case)
//     ($NETWORK:tt, $STORAGE:tt, [  ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//     };
//     // reduce states -> individual state modules
//     ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, [ $STATE:tt $( $STATES:tt)* ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//         #[ macro_use ]
//         #[ allow(non_snake_case) ]
//         mod $STATE {
//             use $crate::*;
//             cross_tests!($NETWORK, $STORAGE, $BLOCK, $STATE, $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//         }
//         cross_tests!($NETWORK, $STORAGE, $BLOCK, [ $($STATES)* ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
//     };
//     // catchall for empty state list (base case)
//     ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, [  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
//     };
//     // base reduction
//     // NOTE: unclear why `tt` is needed instead of `ty`
//     ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, $STATE:tt, $fn_name:ident, $e:expr, keep: $keep:tt, slow: false, args: $($args:tt)*) => {
//
//         type TestType = $crate::TestDescription<
//             common::StaticCommitteeTestTypes,
//             hotshot_testing::TestNodeImpl<
//                 common::StaticCommitteeTestTypes,
//                 hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>,
//                 hotshot_types::data::ValidatingProposal<
//                     common::StaticCommitteeTestTypes,
//                     hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
//                 >,
//                 hotshot_types::vote::QuorumVote<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
//                 $NETWORK<
//                     common::StaticCommitteeTestTypes,
//                     hotshot_types::data::ValidatingProposal<
//                         common::StaticCommitteeTestTypes,
//                         hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
//                     >,
//                     hotshot_types::vote::QuorumVote<common::StaticCommitteeTestTypes,hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
//                     hotshot::traits::election::static_committee::StaticCommittee<
//                         common::StaticCommitteeTestTypes,
//                         hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
//                     >
//                 >,
//                 $STORAGE<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
//                 hotshot::traits::election::static_committee::StaticCommittee<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>
//             >
//         >;
//         cross_test!(TestType, $fn_name, $e, keep: $keep, slow: false, args: $($args)*);
//     };
//     // base reduction
//     // NOTE: unclear why `tt` is needed instead of `ty`
//     ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, $STATE:tt, $fn_name:ident, $e:expr, keep: $keep:tt, slow: true, args: $($args:tt)*) => {
//         #[cfg(feature = "slow-tests")]
//         type TestType = $crate::TestDescription<
//             hotshot_testing::TestNodeImpl<
//                 $STATE,
//                 $STORAGE<$STATE>,
//                 $NETWORK,
//                 hotshot_types::traits::signature_key::ed25519::Ed25519Pub,
//                 hotshot::traits::election::static_committee::StaticCommittee<$STATE>
//             >
//         >;
//
//         cross_test!(TestType, $fn_name, $e, keep: $keep, slow: true, args: $($args)*);
//     };
// }
//
// /// Macro to generate tests for all types based on a description
// /// Arguments:
// /// - $fn_name: a identifier for the outermost test module
// /// - $expr: a TestDescription for the test
// /// - $keep:
// ///   - true is a noop
// ///   - false forces test to be ignored
// #[macro_export]
// macro_rules! cross_all_types {
//     ($fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt) => {
//         #[cfg(test)]
//         #[macro_use]
//         pub mod $fn_name {
//             use $crate::*;
//
//             cross_tests!(
//                 [ MemoryCommChannel ],
//                 [ MemoryStorage ],
//                 [ VDEntryBlock  ],
//                 [ VDEntryState ],
//                 $fn_name,
//                 $e,
//                 keep: $keep,
//                 slow: $slow,
//                 args:
//                 );
//         }
//     };
// }
//
// /// Macro to generate property-based tests for all types based on a description
// /// Arguments:
// /// - $fn_name: a identifier for the outermost test module
// /// - $expr: a TestDescription for the test
// /// - $keep:
// ///   - true is a noop
// ///   - false forces test to be ignored
// /// - $args: list of arguments to fuzz over. The syntax of these must match
// ///          function arguments in the same style as
// ///          <https://docs.rs/proptest/latest/proptest/macro.proptest.html>
// ///          these arguments are available for usage in $expr
// #[macro_export]
// macro_rules! cross_all_types_proptest {
//     ($fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)+) => {
//         #[cfg(test)]
//         #[macro_use]
//         pub mod $fn_name {
//             use $crate::*;
//
//             cross_tests!(
//                 [ MemoryCommChannel ],
//                 [ MemoryStorage ], // AtomicStorage
//                 [ VDEntryBlock  ],
//                 [ VDEntryState ],
//                 $fn_name,
//                 $e,
//                 keep: $keep,
//                 slow: $slow,
//                 args: $($args)+
//                 );
//         }
//     };
// }

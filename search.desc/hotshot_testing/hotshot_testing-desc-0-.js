searchState.loadedDescShard("hotshot_testing", 0, "Testing infrastructure for <code>HotShot</code>\nTest implementation of block builder\nbyzantine framework for tests\ntask that decides when things are complete\ntask that checks leaves received across all nodes from …\nHelpers for initializing system context handle and …\ntask that’s consuming events and asserting safety\npredicates to use in tests\nscripting harness for tests\ntask to spin nodes up and down\nbuilder\nlauncher\nrunner\nthe <code>TestTask</code> struct and associated trait/functions\ntask that’s submitting transactions to the stream\nview generator for tests\ntask for checking if view sync got activated\nEntry for a built block\nHelper function to construct all builder data structures …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nConstruct a tide disco app that mocks the builder API 0.1 …\nConstruct a tide disco app that mocks the builder API 0.1.\nA mock implementation of the builder data source. Builds …\nBuilt blocks\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreate new <code>RandomBuilderSource</code>\nTo get the builder’s address\nTo get the list of available blocks\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nByzantine definitions and implementations of different …\nAn <code>EventTransformerState</code> that multiplies <code>QuorumProposalSend</code>…\nAn <code>EventHandlerState</code> that modifies view number on the …\nAn <code>EventHandlerState</code> that modifies justify_qc on …\nAn <code>EventHandlerState</code> that will send a vote for a bad …\nAn <code>EventHandlerState</code> that modifies view number on the vote …\nAn <code>EventHandlerState</code> that doubles the <code>QuorumVoteSend</code> and …\nView delay configuration\nWhich proposals to be dishonest at\nWhich proposals to be dishonest at\nShared state of all view numbers we send bad proposal at\nShared state with views numbers that leaders were …\nA map that is from view number to vector of events\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nWhen a leader is sending a proposal this method will mock …\nThe view number increment each time it’s duplicatedjust\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nA function passed to <code>NetworkEventTaskStateModifier</code> to …\nThe number of times to duplicate a <code>QuorumProposalSend</code> event\nHow many views the node will be delayed\nSpecify which view number to stop delaying\nHow many times current node has been elected leader and …\nHow many times current node has been elected leader and …\nWhen leader how many times we will send DacSend and …\nStore events from previous views\nNumber added to the original vote’s view number\nHow far back to look for a QC\nCollect all votes the node sends\nCompletion task state\nDescription for a completion task.\nTime-based completion task.\nDescription for a time-based completion task.\nDuration of the task.\nDuration of the task.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nData availability task state\nA map from node ids to <code>NodeMap</code>s; note that the latter may …\nA map from node ids to <code>NodeMapSanitized</code>s; the latter has …\nMap from views to leaves for a single node, allowing …\nA sanitized map from views to leaves for a single node, …\nA view map, sanitized to have exactly one leaf per view.\nphantom marker\nA map from node ids to (leaves keyed on view number)\nwhether we should have seen an upgrade certificate or not\nReturns the argument unchanged.\nHandles an event from one of multiple receivers.\nCalls <code>U::from(self)</code>.\nsafety task requirements\nValidate that each node has only produced one unique leaf …\nValidate that the <code>NodeMap</code> only has a single leaf per view.\nFor a NodeMapSanitized, we validate that each leaf extends …\nfunction used to validate the number of transactions …\ncreate signature\ncreate certificate\nThis function will create a fake <code>View</code> from a provided [<code>Leaf</code>…\nThis function will create a fake <code>View</code> from a provided [<code>Leaf</code>…\ncreate the <code>SystemContextHandle</code> from a node id\ncreate the <code>SystemContextHandle</code> from a node id and …\nTODO: …\nget the keypair for a node id\nThis function permutes the provided input vector <code>inputs</code>, …\ninitialize VID\nsafety violation\nfailure\nin progress\nsuccess\ncross node safety properties\nData availability task state\npossible errors\ncontext for a round TODO eventually we want these to just …\nResult of running a round of consensus\nconvenience type alias for state and block\nthe status of a view\nblock -&gt; # entries decided on that block\nwhether or not to check the block\ncheck if the test failed due to not enough nodes getting …\nwhether or not to check the leaf\nctx\nerror\npass in the views that we expect to fail\nNodes that failed to commit this round\nduring the run view refactor\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\ngenerate leaves\nHandles an event from one of multiple receivers.\nhandles\ninserts an error into the context\ninsert into round result\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nlatest epoch, updated when a leaf with a higher epoch is …\nNOTE: technically a map is not needed left one anyway for …\nnum of total rounds allowed to fail\nrequired number of successful views\nnumber of transactions -&gt; number of nodes reporting that …\nconfigure properties\nresults from previous rounds view number -&gt; round result\nwhether or not the round succeeded (for a custom defn of …\nTransactions that were submitted Nodes that committed this …\nsuccessful views\nsender to test event channel\nthreshold calculator. Given number of live and total …\nwhether or not to check that we have threshold amounts of …\ndetermines whether or not the round passes also do a …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nShared consensus task state\nCurrent epoch\nNumber of blocks in an epoch, zero means there are no …\nThe most recent upgrade certificate this node formed. …\nReturns the argument unchanged.\nThe highest_qc we’ve seen at the start of this task\nThe node’s id\nImmutable instance state\nCalls <code>U::from(self)</code>.\nLatest view number that has been proposed for.\nMembership for Quorum Certs/votes\nOur Private Key\nTable for the in-progress proposal dependency tasks.\nOur public key\nThis node’s storage ref\nView timeout from config.\nLock for a decided upgrade\nReference to consensus. The replica will require a write …\nThe consensus metrics\nIn-progress DRB computation task.\nNumber of blocks in an epoch, zero means there are no …\nReturns the argument unchanged.\nThe node’s id\nImmutable instance state\nCalls <code>U::from(self)</code>.\nLatest view number that has been voted for.\nMembership for Quorum certs/votes and DA committee …\nThe underlying network\nOutput events to application\nPrivate Key.\nPublic key.\nReference to the storage.\nLock for a decided upgrade\nTable for the in-progress dependency tasks.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nThe time to wait on the receiver for this script.\ndenotes a change in node state\nspin the node down\nspin the node’s network down\nspin the node’s network up\nSpin the node up or down\nTake a node down to be restarted after a number of views\nStart a node up again after it’s been shutdown for …\nSpinning task state\ndescription of the spinning task (used to build a spinning …\nconvenience type for state and block\nspin the node up\nAdd specified delay to async calls\ntime based changes\nGenerate network channel for restart nodes\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nhandle to the nodes\nHighest qc seen in the test for restarting nodes\nthe index of the node\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nLast decided leaf that can be used as the anchor leaf to …\nlate start nodes\nmost recent view seen by spinning task\nNext epoch highest qc seen in the test for restarting nodes\nthe changes in node status, time -&gt; changes\nContext stored for nodes to be restarted with\nspin the node or node’s network up or down\nDescribes a possible change to builder status during test\nMetadata describing builder behaviour during a test\nmetadata describing a test\ndata describing how a round should be timed.\nDelay config if any to add delays to asynchronous calls\nnodes with byzantine behaviour\nThe maximum amount of time a leader can wait to get a …\ndescription of builders to run\nview number -&gt; change to builder status\ncompletion task\nSize of the staked DA committee for the test\ntime to wait until we request data associated with a …\nby default, just a single round\nDefault setting with 20 nodes and 8 views of successful …\nthe default metadata for multiple rounds\nthe default metadata for a stress test\nThe rate at which errors occur in the mock solver API\ndescription of fallback builder to run\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nturn a description of a test (e.g. a <code>TestDescription</code>) into …\nturn a description of a test (e.g. a <code>TestDescription</code>) into …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nBase duration for next-view timeout, in milliseconds\nnumber of bootstrap nodes (libp2p usage only)\nTotal number of staked nodes in the test\noverall safety property description\nDelay before sending through the secondary network in …\nWhether to skip initializing nodes that will start late, …\ndescription of the solver to run\nspinning properties\nnodes available at start\nwhether to initialize the solver on startup\ntiming data\ntxns timing\nunrelabile networking metadata\nview in which to propose an upgrade\nboxed closure used to validate the resulting transactions\nview sync check task\nview sync timeout\nWrapper for a function that takes a <code>node_id</code> and returns an …\nA type alias to help readability\ngenerators for resources used by each node\ntest launcher\nany additional test tasks to run\ngenerate channels\nconfiguration used to generate each hotshot node\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nlaunch the test\ngenerate a new marketplace config for each node\nmetadata used for tasks\nModifies the config used when generating nodes with <code>f</code>\ngenerator for resources\ngenerate new storage for each node\nconfig that contains the signature keys\nThe system context that we’re passing directly to the …\nThe late node context dictates how we’re building a node …\nThis type combines all of the parameters needed to build …\nA yet-to-be-started node that participates in tests\na node participating in a test\nThe node is to be restarted so we will build the context …\nThe runner of a test network spin up and down nodes, …\nThe system context that we’re passing to the node when …\nPhantom for N\nadd a specific node with a config\nadd a specific node with a config\nAdd nodes.\nAdd auction solver.\nThe config associated with this node.\nEither the context to which we will use to launch HotShot …\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nThe handle to the node’s internals\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nnodes with a late start\ntest launcher, contains a bunch of useful metadata and …\nThe marketplace config for this node.\nThe memberships of this particular node.\nThe underlying network belonging to the node\nThe underlying network belonging to the node\nthe next node unique identifier\nThe node’s unique identifier\nnodes in the test\nexecute test\nthe solver server running in the test\nThe storage trait for Sequencer persistence.\nType alias for type-erased <code>TestTaskState</code> to be used for …\nType of event sent and received by the task\nthe test task failed with an error\nthe test task passed\nenum describing how the tasks completed\nA basic task which loops waiting for events to come from …\nType for mutable task state that can be used as the state …\nAdd the network task to handle messages and publish events.\nCheck the result of the test.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nHandles an event from one of multiple receivers.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreate a new task\nReceives events that are broadcast from any task, …\nSpawn the task loop, consuming self.  Will continue until …\nThe state of the task.  It is fed events from <code>event_sender</code> …\nReceiver for test events, used for communication between …\nTODO\nsubmit transactions in a round robin style using every …\nstate of task that decides when things are completed\nbuild the transaction task\ntime to wait between txns\nReturns the argument unchanged.\nReturns the argument unchanged.\nHandles for all nodes.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nOptional index of the next node.\nReceiver for the shutdown signal from the testing harness\nAdvances to the next view by skipping the current view and …\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nMoves the generator to the next view by referencing an …\ndon’t care if the node should hit view sync\nthe node should not hit view sync\nenum desecribing whether a node should hit view sync\n(min, max) number nodes that may hit view sync, inclusive\n<code>ViewSync</code> task state\nDescription for a view sync task.\n<code>ViewSync</code> Task error\nthe node should hit view sync\nPhantom data for TYPES and I\nproperties of task\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nHandles an event from one of multiple receivers.\nnodes that hit view sync\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.")
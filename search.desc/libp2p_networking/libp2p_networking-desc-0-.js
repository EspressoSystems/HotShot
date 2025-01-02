searchState.loadedDescShard("libp2p_networking", 0, "Library for p2p communication\nNetwork logic\nsymbols needed to implement a networking instance over …\nadd vec of known peers or addresses\nan autonat event\nStart the bootstrap process to kademlia\n<code>BoxedTransport</code> is a type alias for a boxed tuple …\nActions to send from the client to the swarm\nThe number of connected peers has possibly changed\nThe default Kademlia replication factor\na DHT event\na direct message event\nclient request to send a direct serialized message\nRecv-ed a direct message from a node\nclient request to send a direct reply to a message\nRecv-ed a direct response from a node (that hopefully was …\nRequest the number of connected peers\nRequest the set of connected peers\nGet(Key, Chan)\nPrint the routing  table to stderr, debugging only\nConfiguration for Libp2p’s Gossipsub\na gossip  event\nbroadcast a serialized message\nRecv-ed a broadcast\nan identify event. Is boxed because this event is much …\nIgnore peers. Only here for debugging purposes. Allows us …\nReport that kademlia has successfully bootstrapped into …\nGet address of peer\nOverarching network behaviour performing:\nevents generated by the swarm that we wish to relay to the …\ninternal representation of the network events only used …\nNetwork definition\ndescribe the configuration of the network\nBuilder for <code>NetworkNodeConfig</code>.\nError type for NetworkNodeConfigBuilder\nA handle containing:\ninternal network node receiver\nprune a peer\nPut(Key, Value) into DHT relay success back on channel\nConfiguration for Libp2p’s request-response\nkill the swarm\nsubscribe to a topic\nUninitialized field\nunsubscribe from a topic\nCustom validation error\nThe signed authentication message sent to the remote peer …\nThe signed authentication message sent to the remote peer …\nAuto NAT behaviour to determine if we are publicly …\nnetworking behaviours wrapping libp2p’s behaviours\nThe address to bind to\nThe address to bind to\nForked <code>cbor</code> codec with altered request/response sizes\ndefines the swarm and network definition (internal)\nThe DHT store. We use a <code>FileBackedStore</code> to occasionally …\nThe path to the file to save the DHT to\nThe path to the file to save the DHT to\nHandler for DHT Events\nThe timeout for DHT lookups.\nThe timeout for DHT lookups.\npurpose: directly messaging peer\nHandler for direct messages\nThe time period that messages are stored in the cache\nTime to live for fanout peers\nIf enabled newly created messages will always be sent to …\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nBind all interfaces on port <code>port</code> NOTE we may want …\nGenerates an authenticated transport checked against the …\nConfiguration for <code>GossipSub</code>\nConfiguration for <code>GossipSub</code>\nAffects how many peers we will emit gossip to at each …\nMinimum number of peers to emit gossip to during a …\nControls how many times we will allow a peer to request …\npurpose: broadcasting messages to many peers NOTE …\nInitial delay in each heartbeat\nThe heartbeat interval\nThe number of past heartbeats to gossip about\nThe number of past heartbeats to remember the full …\nhuman readable id\npurpose: identifying the addresses from an outside POV\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nTime to wait for a message requested through IWANT …\nThe keypair for the node\nThe keypair for the node\nthe local address we’re listening on\nthe listener id we are listening on, if it exists\nThe maximum number of messages in an IHAVE message\nMaximum number of IHAVE messages to accept from a peer …\nThe maximum number of messages we will process in a given …\nThe maximum gossip message size\nThe target number of peers in the mesh\nThe maximum number of peers in the mesh\nThe minimum number of peers in the mesh\nThe minimum number of mesh peers that must be outbound\nnetwork configuration\nfunctionality of a libp2p network node\nthe peer id of the networkbehaviour\npeer id of network node\nCache duration for published message IDs\nthe receiver\nkill switch\nReplication factor for entries in the DHT\nReplication factor for entries in the DHT\nrepublication interval in DHT, must be much less than <code>ttl</code>\nrepublication interval in DHT, must be much less than <code>ttl</code>\nConfiguration for <code>RequestResponse</code>\nConfiguration for <code>RequestResponse</code>\nThe maximum request size in bytes\nChannel to resend requests, set to Some when we call …\nThe maximum response size in bytes\nsend an action to the networkbehaviour\nSpawn a network node task task and return the handle and …\nThe stake table. Used for authenticating other nodes. If …\nThe stake table. Used for authenticating other nodes. If …\nthe swarm of networkbehaviours\nlist of addresses to connect to at initialization\nlist of addresses to connect to at initialization\nAlternative Libp2p transport implementations\nexpiratiry for records in DHT\nexpiratiry for records in DHT\nmsg contents\nKey to publish under\nKey to search for\nChannel to notify caller of result of publishing\nChannel to notify caller of value (or failure to find …\npeer id\nnumber of retries\nnumber of retries to make\nValue to publish under\nWrapper around Kademlia\nWrapper around <code>RequestResponse</code>\nexponential backoff type\nState of bootstrapping\nBehaviour wrapping libp2p’s kademlia included:\nDHT event enum\nrepresents progress through DHT\nThe query has been started\nOnly event tracked currently is when we successfully …\nMetadata holder for get query\nMetadata holder for get query\nthe maximum number of nodes to query in the DHT at any one …\nthe number of nodes required to get an answer from in …\nNot in progress\nThe query has not been started\nIn progress\nState used for random walk and bootstrapping\nRetry timeout\nExponential retry backoff\nExponential retry backoff\nTask for doing bootstraps at a regular interval\nState of bootstrapping\nSender to the bootstrap task\nhandle a DHT event\nSend that the bootstrap succeeded\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nRetrieve a value for a key from the DHT.\nupdate state based on recv-ed get query\nUpdate state based on put query\nin progress queries for nearby peers\nList of in-progress put requests\nList of in-progress get requests\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nthe key to look up\nthe key to put\nCreate a new DHT behaviour\nnotify client of result\nnotify client of result\nnumber of replicas required to replicate over\nThe lookup keys for all outstanding DHT queries\nthe peer id (useful only for debugging right now)\nPhantom type for the key\nprint out the routing table to stderr\nprogress through DHT query\nprogress through DHT query\nPublish a key/value to the kv store. Once replicated upon …\nAdditional DHT record functionality\nalready received records\nGet the replication factor for queries\nreplication factor\nthe number of remaining retries before giving up\nSpawn a task which will retry the query after a backoff.\nSpawn a task which will retry the query after a backoff.\nSender to retry requests.\nSets a sender to bootstrap task\nGive the handler a way to retry requests.\nState of bootstrap\nAdditional DHT store functionality\nthe value to put\nBootstrap has finished\nBootstrap task’s state\nInternal bootstrap events\nShutdown bootstrap\nStart bootstrap\nStart bootstrap\nReturns the argument unchanged.\nReturns the argument unchanged.\nField indicating progress state\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nTask’s sender\nRun bootstrap task\nTask’s loop\nTask’s receiver\nA namespace for looking up P2P identities\nThe namespace of a record. This is included with the key …\nA record’s key. This is a concatenation of the namespace …\nA (signed or unsigned) record value to be stored …\nA signed record value\nAn unsigned record value\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nThe actual key\nThe namespace of the record key\nCreates and returns a new unsigned record\nCreate and return a new record key in the given namespace\nCreates and returns a new signed record by signing the key …\nRequire certain namespaces to be authenticated\nConvert the record key to a byte vector\nTry to convert a byte vector to a record key\nIf the message requires authentication, validate the …\nGet the underlying value of the record\nThis file contains the <code>FileBackedStore</code> struct, which is a …\nThis file contains the <code>ValidatedStore</code> struct, which is a …\nA <code>RecordStore</code> wrapper that occasionally saves the DHT to a …\nA serializable version of a Libp2p <code>Record</code>\nThe record expiration time in seconds since the Unix epoch\nReturns the argument unchanged.\nReturns the argument unchanged.\nApproximate an <code>Instant</code> to the number of seconds since the …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nThe key of the record\nThe maximum number of records that can be added to the …\nCreate a new <code>FileBackedStore</code> with the given underlying …\nThe path to the file\nThe (original) publisher of the record.\nOverwrite the <code>put</code> method to potentially save the record to …\nThe running delta between the records in the file and the …\nOverwrite the <code>remove</code> method to potentially remove the …\nAttempt to restore the DHT to the underlying store from …\nAttempt to save the DHT to the file at the given path\nThe underlying store\nConvert a unix-second timestamp to an <code>Instant</code>\nThe value of the record\nA <code>RecordStore</code> wrapper that validates records before …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCreate a new <code>ValidatedStore</code> with the given underlying store\nPhantom type for the key\nOverwrite the <code>put</code> method to validate the record before …\nThe underlying store\nWrapper metadata around libp2p’s request response usage: …\nLilst of direct message output events\nRequest to direct message a peert\nWe received as Direct Request\nWe received a Direct Response\nAdd a direct request for a given peer\nbackoff since last attempted request\nthe data\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nhandle a direct message event\nIn progress queries\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nthe recv-ers peer id\nthe number of remaining retries before giving up\nTrack (with exponential backoff) sending of some sort of …\nfactor to back off by\nMarked as expired regardless of time left.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nWhether or not the timeout is expired\nCreate new backoff\nReturn the timeout duration and start the next timeout.\nreset backoff\nValue to reset to when reset is called\nstart next timeout result: whether or not we succeeded if …\nwhen we started the timeout\nthe current timeout amount\n<code>Behaviour</code> type alias for the <code>Cbor</code> codec\nForked <code>cbor</code> codec with altered request/response sizes\nConvert a <code>cbor4ii::serde::DecodeError</code> into an <code>io::Error</code>\nConvert a <code>cbor4ii::serde::EncodeError</code> into an <code>io::Error</code>\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCreate a new <code>Cbor</code> codec with the given request and …\nPhantom data\nMaximum request size in bytes\nMaximum response size in bytes\nOverarching network behaviour performing:\nAdd an address\nAdd a direct request for a given peer\nAdd a direct response for a channel\nAuto NAT behaviour to determine if we are publicly …\nThe DHT store. We use a <code>FileBackedStore</code> to occasionally …\npurpose: directly messaging peer\nReturns the argument unchanged.\npurpose: broadcasting messages to many peers NOTE …\npurpose: identifying the addresses from an outside POV\nCalls <code>U::from(self)</code>.\nCreate a new instance of a <code>NetworkDef</code>\nPublish a given gossip\nSubscribe to a given topic\nUnsubscribe from a given topic\nWrapped num of connections\nNumber of connections to a single peer before logging an …\nMaximum size of a message\nNetwork definition\ninitialize the DHT with known peers add the peers to …\nconfiguration for the libp2p network (e.g. how it should …\nreturn hashset of PIDs this node is connected to\nHandler for DHT Events\nHandler for direct messages\nReturns the argument unchanged.\nlibp2p network handle allows for control over the libp2p …\nevent handler for client events currently supported …\nevent handler for events emitted from the swarm\nCalls <code>U::from(self)</code>.\nthe listener id we are listening on, if it exists\nCreates a new <code>Network</code> with the given settings.\nReturns number of peers this node is connected to\nGet a reference to the network node’s peer id.\npeer id of network node\nPublish a key/value to the record store.\nChannel to resend requests, set to Some when we call …\nSpawn a task to listen for requests on the returned channel\nstarts the swarm listening on <code>listen_addr</code> and optionally …\nthe swarm of networkbehaviours\nThe default Kademlia replication factor\nConfiguration for Libp2p’s Gossipsub\ndescribe the configuration of the network\nBuilder for <code>NetworkNodeConfig</code>.\nError type for NetworkNodeConfigBuilder\nConfiguration for Libp2p’s request-response\nUninitialized field\nCustom validation error\nThe signed authentication message sent to the remote peer …\nThe signed authentication message sent to the remote peer …\nThe signed authentication message sent to the remote peer …\nThe address to bind to\nThe address to bind to\nThe address to bind to\nBuilds a new <code>NetworkNodeConfig</code>.\nCreate an empty builder, with all fields set to <code>None</code> or …\nThe path to the file to save the DHT to\nThe path to the file to save the DHT to\nThe path to the file to save the DHT to\nThe timeout for DHT lookups.\nThe timeout for DHT lookups.\nThe timeout for DHT lookups.\nThe time period that messages are stored in the cache\nTime to live for fanout peers\nIf enabled newly created messages will always be sent to …\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nConfiguration for <code>GossipSub</code>\nConfiguration for <code>GossipSub</code>\nConfiguration for <code>GossipSub</code>\nAffects how many peers we will emit gossip to at each …\nMinimum number of peers to emit gossip to during a …\nControls how many times we will allow a peer to request …\nInitial delay in each heartbeat\nThe heartbeat interval\nThe number of past heartbeats to gossip about\nThe number of past heartbeats to remember the full …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nTime to wait for a message requested through IWANT …\nThe keypair for the node\nThe keypair for the node\nThe keypair for the node\nThe maximum number of messages in an IHAVE message\nMaximum number of IHAVE messages to accept from a peer …\nThe maximum number of messages we will process in a given …\nThe maximum gossip message size\nThe target number of peers in the mesh\nThe maximum number of peers in the mesh\nThe minimum number of peers in the mesh\nThe minimum number of mesh peers that must be outbound\nCache duration for published message IDs\nReplication factor for entries in the DHT\nReplication factor for entries in the DHT\nReplication factor for entries in the DHT\nrepublication interval in DHT, must be much less than <code>ttl</code>\nrepublication interval in DHT, must be much less than <code>ttl</code>\nrepublication interval in DHT, must be much less than <code>ttl</code>\nConfiguration for <code>RequestResponse</code>\nConfiguration for <code>RequestResponse</code>\nConfiguration for <code>RequestResponse</code>\nThe maximum request size in bytes\nThe maximum response size in bytes\nThe stake table. Used for authenticating other nodes. If …\nThe stake table. Used for authenticating other nodes. If …\nThe stake table. Used for authenticating other nodes. If …\nlist of addresses to connect to at initialization\nlist of addresses to connect to at initialization\nlist of addresses to connect to at initialization\nexpiratiry for records in DHT\nexpiratiry for records in DHT\nexpiratiry for records in DHT\nA handle containing:\ninternal network node receiver\nTell libp2p about known network nodes\nNotify the network to begin the bootstrap process\nReturn a reference to the network config\nreturn hashset of PIDs this node is connected to\nMake a direct request to <code>peer_id</code> containing <code>msg</code>\nMake a direct request to <code>peer_id</code> containing <code>msg</code> without …\nReply with <code>msg</code> to a request over <code>chan</code>\nReturns the argument unchanged.\nReturns the argument unchanged.\nReceive a record from the kademlia DHT if it exists. Must …\nGet a record from the kademlia DHT with a timeout\nGossip a message to peers\nGossip a message to peers without serializing\nGet a reference to the network node handle’s id.\nhuman readable id\nIgnore <code>peers</code> when pruning e.g. maintain their connection\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nGet a reference to the network node handle’s listen addr.\nthe local address we’re listening on\nLooks up a node’s <code>PeerId</code> by its staking key. Is …\nLook up a peer’s addresses in kademlia NOTE: this should …\nnetwork configuration\nReturns number of peers this node is connected to\nGet a reference to the network node handle’s peer id.\nthe peer id of the networkbehaviour\nPrint out the routing table used by kademlia NOTE: only …\nForcefully disconnect from a peer\nInsert a record into the kademlia DHT\nInsert a record into the kademlia DHT with a timeout\nthe receiver\nrecv a network event\nkill switch\nsend an action to the networkbehaviour\nSend a client request to the network\nAdd a kill switch to the receiver\nCleanly shuts down a swarm node This is done by sending a …\nSpawn a network node task task and return the handle and …\nSubscribe to a topic\nTake the kill switch to allow killing the receiver task\nUnsubscribe from a topic\nWait until at least <code>num_peers</code> have connected\nThe timeout for the authentication handshake. This is used …\nA helper trait that allows us to access the underlying …\nThe deserialized form of an authentication message that is …\nThe maximum size of an authentication message. This is …\nA wrapper for a <code>Transport</code> that bidirectionally …\nA type alias for the future that upgrades a connection to …\nGet a mutable reference to the underlying connection\nGet a mutable reference to the underlying <code>PeerId</code>\nA pre-signed message that we send to the remote peer for …\nProve to the remote peer that we are in the stake table by …\nCreate an sign an authentication message to be sent to the …\nDial a remote peer. This function is changed to perform an …\nReturns the argument unchanged.\nReturns the argument unchanged.\nWrap the supplied future in an upgrade that performs the …\nThe underlying transport we are wrapping\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreate a new <code>StakeTableAuthentication</code> transport that wraps …\nPhantom data for the connection type\nThe encoded peer ID of the sender. This is appended to the …\nThis function is where we perform the authentication …\nThe encoded (stake table) public key of the sender. This, …\nA helper function to read a length-delimited message from …\nThe below functions just pass through to the inner …\nThe signature on the public key\nThe stake table we check against to authenticate …\nValidate the signature on the public key and return it if …\nVerify that the remote peer is:\nA helper function to write a length-delimited message to a …\nRepresentation of a Multiaddr.\nIdentifier of a peer of the network.\nA channel for sending a response to an inbound request.\nCreate a new, empty multiaddress.\nChecks whether the given <code>Multiaddr</code> is a suffix of this …\nConvert a Multiaddr to a string\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nParses a <code>PeerId</code> from bytes.\nTries to turn a <code>Multihash</code> into a <code>PeerId</code>.\nBuilds a <code>PeerId</code> from a public key.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns true if the length of this multiaddress is 0.\nChecks whether the response channel is still open, i.e. …\nReturns the components of this multiaddress.\nReturn the length in bytes of this multiaddress.\nPops the last <code>Protocol</code> of this multiaddr, or <code>None</code> if the …\nReturns &amp;str identifiers for the protocol names themselves.\nAdds an already-parsed address component to the end of …\nGenerates a random peer ID from a cryptographically secure …\nReplace a <code>Protocol</code> at some position in this <code>Multiaddr</code>.\nReturns a base-58 encoded string of this <code>PeerId</code>.\nReturns a raw bytes representation of this <code>PeerId</code>.\nReturn a copy of this <code>Multiaddr</code>’s byte representation.\nLike <code>Multiaddr::push</code> but consumes <code>self</code>.\nCreate a new, empty multiaddress with the given capacity.\nAppends the given <code>PeerId</code> if not yet present at the end of …")
# Introduction
TODO: Blurb about BFT and background

## Scaling difficulties with previous BFT protocols

Previous BFT Protocols have struggled to effectively address the scaling challenges introduced in the blockchain space,
performing well in the permissioned consensus scenario with a small number of nodes participating, but becoming strongly
bottlenecked by the low throughput of gossip protocols in a more decentralized 
(TODO: Citiation, try to find paper about the TPS rates of various chains in practice), with many blockchains hitting
practical limits well below the demands typically placed on modern financial infrastructure. 

Our solution primarily builds upon the work of two previous protocols, HotStuff and Algorand (TODO: Cite), providing a
a synthesis of their approaches for reducing overhead and increasing throughput:

- Committee election (Algorand)
  Committee election provides a reduction in constants by engaging only a randomly chosen subset of the network for any
  given step
- Linear complexity leader failover (HotStuff)
- Optimistic responsiveness (HotStuff):
  Does not have to wait for a delay to make progress, the network can proceed to the next block as soon as a block is
  finalized
  
Our system also provides further improvements, by integrating ZK rollups to separate data availability from consensus,
allowing the data availability protocol to take advantage of full WAN speeds, sending only small, constant size messages
over the gossip protocol.

# Related Work

TODO: Overview covering:

 - HotStuff
 - Algorand
 - Celestia
 - Anything else? Check the lit 
 
# Model 

Consider a system consisting of an arbitrary number of replicas, collectively assigned $n = 3f + \epsilon$ units of
stake. A subset of up to $f$ units of stake ($F$) are assigned to byzantine faulty replicas, while the remaining stake
is assigned to honest replicas.

Network communication, for the proposes of analysis, is assumed to be authenticated and reliable: A message is only
received by the intended set of precipitants, and all replicas are assumed to check message authenticity. The contents
of a message, such as the contained quorum certificate or VRF vote, are also verified before a message is accepted. The
following two network operations are utilized:

- _Direct Message_:
  A message is sent directly to a specific other replica
- _Broadcast_:
  A message is sent to all other replicas on the network
  
In the style of HotStuff, from which this protocol is derived, the _partial synchrony model_ of Dwork et al (TODO:
proper cite) is used, in which a known time bound ($\Delta$), and an unknown Global Stabilization Time (GST) are
assumed, such that all messages will be delivered within $\Delta$ after the GST has elapsed.
  
Our protocol preserves safety with extremely high probability regardless of network conditions, and guarantees liveness
after the GST has elapsed (during the GST no guarantee of progress can be made). In practice, the protocol will continue
to make progress during periods of network stability (messages arrive within $\Delta$), though it simplifies the
analysis to assume that the network remains stable forever after the GST has elapsed.

## Cryptographic Primitives

HotShot relies upon the use of a _Verifiable Random Function_ (VRF) (TODO: cite) to perform cryptographic sortition
and voting. The VRF is a function of a private key and some public information from the blockchain, and can be verified
using the same public information and the matching public key.

## Complexity 

TODO: explain authenticator complexity, this is basically the same as hotstuff

# HotShot Protocol

The HotShot protocol solves the problem of Byzantine Fault Tolerant State Machine replication, by running a variant of
the Chained HotStuff protocol among subset of the overall collection of replicas selected via cryptographic sortition. 

The protocol operates in a sequence of monotonically increasing numbered _views_, advancing blocks one step further down
the pipeline in each view number, with 3 blocks in the pipeline at any given time. Each view has a unique leader, with
the leader for each view number known in advance by all replicas.

Each replica internally stores a _tree_ of pending blocks, each node containing the proof of validity for a proposed
command, protocol metadata, and a parent link. The _branch_ of a given node is the path from a node, following the
parent links, all the way up to the root node, containing the genesis block.

```{.rust .numberLines}
struct Proof {
    /// The hash of the command associated with this node
    hash: Hash,
    /// The proof of availability for the command with that hash
    proof: ZKProof,
}

struct Node {
    /// Pointer to this node's parent
    parent: Node,
    /// The proof of validity for the command associated with this node
    command_proof: Proof,
    /// The view number this node is from
    view_number: u64,
    /// The justifying quorum certificate for this Node
    justify: QC,
}

fn create_node(parent: Node, command_proof: Proof, justify: QC) -> Node {
    Node {
        parent,
        command_proof,
        view_number: current_view_number(),
        justify,
    }
}
```

Over the sequence of views, and increasing chain of _committed_ nodes is grown, defined as those nodes which are included
in a valid _three chain_.

For simplicity of analysis, each individual unit of stake is assumed to correspond to a single vote attempt (e.g., a replica
with 20 units of stake is allocated 20 chances to serve on the committee for each given round)

A _Quorum Certificate_ (QC) contains the hash of the item being voted on, as well as at least the threshold number of
VRF signatures (votes) for that item, with the threshold being chosen based on the expected committee size to provide an
acceptable level of probabilistic security (see analysis section for details).

A vote is attempted by producing a VRF signature of the item being voted on, and the vote is valid if the resulting VRF
output is lower than a certain threshold value, arbitrarily chosen to tune the average committee size (see analysis
section for details)

## One, Two, and Three Chains

(TODO: Diagram) 

When a node $b$ contains a link to a direct parent from the proceeding view number, it is said to have a one-chain

If $b$'s parent, denoted $b'$, forms a One-chain, then $b$ forms a two chain.

If $b'$ forms a two chain, then $b$ forms a three chain

## Algorithm

The algorithm for the protocol is as follows:

```{.rust .numberLines}
for view_number in 1.. {
    if is_leader(view_number) {
        // Logic for if this node is the leader
        let messages = get_messages(); // Collect votes that have been sent in for the proposal from the previous round
    } else {
        // Logic for if this node is a replica
    }
}
```

## Utilites

TODO: Replace the rust-like psuedocode with proper algorithim2e 

Utilities for the Algorithm are as follows:

```{.rust .numberLines}
struct QC {
    view_number: ViewNumber,
    node: Leaf,
    sig: FullSignature,
}

impl QC {
    /// Turns a collection of votes into a quorum certificate
    fn new(votes: Collection<Votes>) -> QC,
}

/// Holds the protocol state for a single replica
struct HotShot {
    generic_qc: QC,
    locked_qc: QC,
    /// Most recently comitted state
    state: State,
    /// Some public/private state pair suitable for use with a vrf
    vrf_state: VRFState,
    /// Set of messages collected in the previous round if we were the leader
    leader_messages: (Collection<Message>, ViewNumber),
    /// The id of this replica
    id: ReplicaId,
}

impl HotShot {
    fn safe_node(&self, node: Leaf) -> Bool {
        let qc = node.justify;
        node.extends_from(self.locked_qc.node) || // Safety rule
            qc.view_number() > self.locked_qc.view_number() // Liveness rule
    }
}

struct VRFSlot {
    ...
}

impl VRFSlot {
    fn partial_sign(x: Any) -> PartialSignture;
}

/// Takes the nodes VRF state and combines it with the current committed state
/// and view number, producing a collection of VRFSlots representing the nodes
/// seats on the current committee
fn get_slots(vrf_state: VRFState, comitted_state: State, view_number: ViewNumber) -> Collection<VRFSlot>;

/// Returns the leader for the given view_number
fn leader(view_number) -> ReplicaId;

enum Message {
    Proposal {
        view_number: ViewNumber,
        node: Leaf,
    }
    VoteType<TYPES>{
        view_number: ViewNumber,
        node: Leaf,
        partial_sig: PartialSignature,
    }
}

/// Broadcasts a message to the network
fn broadcast(message: Message);

/// Sends a message directly to a node
fn direct(message: Message, node: RepilcaId);

/// Returns the proposal message from the leader matching the provided view number
async fn leader_proposal(view_number);
```

The protocol is defined as follows:

```{.rust .numberLines} 
let self: HotShot;
for view_number in 1.. {
    let leader = leader(view_number);
    let next_leader = leader(view_number + 1); 
    if self.id == leader {
        // As a leader
        let high_qc = self.leader_messages.max_by(justify.view_number).justify;
        if high_qc.view_number() > self.generic_qc.view_number() {
            self.generic_qc = high_qc;
        }
        let proposal = Leaf::new(self.generic_qc.node, command, self.generic_qc);
        broadcast(Message::Proposal{view_number, node: proposal});
    } else {
        let m = leader_proposal(view_number).await;
        let b_star = m.node;
        let b_prime_prime = b_star.justify.node;
        let b_prime = b_prime_prime.justify.node;
        let b = b_prime.justify.node;
        if self.safe_node(b_star) {
            // Get the vrf slots we have for this round
            let slots = get_slots(self.vrf_state, self.state, view_number);
            let partial_sig = slot.partial_sign((view_number,b_star));
            // Produce a partial signature for each slot we hold and send it to
            // the leader for the next round
            for slot in slots {
                direct(
                    Message::Vote{
                        view_number,
                        node: b_star,
                        partial_sig,
                    },
                    next_leader,
                );
            }
        }
        // Start pre-commit for b_star's parent
        if b_star.parent == b_prime_prime {
            self.generic_qc = b_star.justify;
            // Start commit for b_star's grandparent
            if b_prime_prime.parent == b_prime {
                self.locked_qc = b_prime_prime.justify;
                // Start decide for b_star's great-grandparent
                if b_prime.parent == b {
                    // execute state transitions between current committed state and b
                }
            }
        }
    }
    
    // As the leader for the next round
    if self.id == next_leader {
        // Wait for all the messages to come in until there are at least n - f votes
        let m = wait_until(inbound_messages.votes.len() >= n - f).await;
        self.generic_qc = QC::new(m.votes);
    }
}
```

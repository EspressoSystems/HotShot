## QuorumProposalRecv

![QuorumProposalRecv](/docs/diagrams/images/HotShotFlow-QuorumProposalRecv.drawio.png "QuorumProposalRecv")

#### Notes
* We emit a `ViewChange` event if we've seen evidence for a view change *and* if the quorum certificate on the proposal is valid.  We could optionally emit a `ViewChange` event if we've seen evidence but the quorum certificate on the proposal is invalid, but that is not the choice we make here.  Either choice is correct, but the former is simpler.  
* We include a view sync certificate as view change evidence.  This isn't strictly required, but it makes the view change logic more understandable and avoids an attack where a view sync relay only sends a commit certificate to the next leader, resulting in other nodes rejecting the leader's proposal because it doesn't have proper view change evidence. It also signals nodes to handle view sync certificate evidence, which is handled differently than timeout certificate evidence or quorum certificate evidence. 
* We never emit a `QuorumProposalValidated` event if we are missing previous data.  It is unsafe to emit this event if we can't properly validate the entire proposal. 
* This diagram represents the [HotStuff-2](https://eprint.iacr.org/2023/397.pdf) protocol

# VoteOnQuorumProposal

![VoteOnQuorumProposal](/docs/diagrams/images/HotShotFlow-VoteOnQuorumProposal.drawio.png "VoteOnQuorumProposal")

## Mutually Verify Share, Cert, and Proposal
* Nodes must verify that the `OptimisticDaCertificate`, their `VidShare`, and the `QuorumProposal` all commit to the same data.  It is only possible to do this verification once we've received all the necessary data.  
* The event/task dependency infrastructure should only spawn this task if the view for all 3 dependencies is the same. It is an internal error if this is not the case. 

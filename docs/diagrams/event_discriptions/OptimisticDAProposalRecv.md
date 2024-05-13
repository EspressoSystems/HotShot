# OptimisticDAProposalRecv

![OptimisticDAProposalRecv](/docs/diagrams/images/HotShotFlow-OptimisticDAProposalRecv.drawio.png "QuorumProposalRecv")

## Basic Message Validation
* It is possible for some applications built on top of HotShot to listen for DA proposals but not vote on them (such as builders).  

## DA Proposal Validation and Processing
* "Valid" DA proposal data is a no-op for now.  More validation is done in the `VoteOnQuorumProposal` task. 

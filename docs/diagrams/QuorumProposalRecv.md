# QuorumProposalRecv

![QuorumProposalRecv](/docs/diagrams/images/HotShotFlow-QuorumProposalRecv.drawio.png "QuorumProposalRecv")


## Basic Message Validation


## View Change Evidence
* There are 3 types of view change evidence: 
  1. A valid `QuorumCertificate` for the immediately preceding view
  2. A valid `TimeoutCertificate` for the immediately preceding view
  3. A valid `ViewSyncCommitCertificate` for the current view
* It isn't not strictly necessary to include `ViewSyncCommitCertificate`s in the `QuorumPropsoal`, but we choose to do so for the following reason: 
  * If `ViewSyncCommitCertificate`s are not included in the `QuorumProposal` it is possible for honest nodes to reject valid proposals.  For example, a view leader may receive the `ViewSyncCommitCertificate` from the view sync relay, update its local view, and send a `QuorumProposal` for the new view.  But other nodes may not have received the `ViewSyncCommitCertificate` yet (either through genuine network delays or Byzantine behavior by the view sync relay).  Thus, the other nodes will not be able to process the `QuorumProposal` since they do not have evidence the network has advanced to the view in the `ViewSyncCommitCertificate`.  These nodes would either need to reject the proposal outright, or wait to process the proposal until they receive the `ViewSyncCommitCertificate`. The former behavior would cause unnecessary view failures, and the latter would add complexity to the code.  Including the `ViewSyncCommitCertificate` in the `QuorumProposal` addresses both these concerns. 
* It is possible for a node to receive multiple `ViewSyncCommitCertificate`s: one from the view sync relay and one from the view leader.  Therefore processing this certificate must be idempotent. 
* It is possible (and valid) for a node to receive valid view change evidence but an invalid `QuorumProposal`.  There are several ways to handle this scenario, but we choose to allow nodes to update their `latest_known_view` even if other validation of the proposal fails.  This helps the network maintain view synchronization in spite of invalid proposals. 
* It is possible for a `QuorumProposal` to have valid view change evidence from a `TimeoutCertificate` or `ViewSyncCommitCertificate`, but to have an invalid `QurorumCertificate`.  It would be valid to update the view in this case, since there exists valid view change evidence.  But for simplicity, we choose not to.  If view change evidence is valid but the `QuorumCertificate` associated with the proposal is invalid nodes will not update their view. 

## Proposal Validation
* This proposal validation represents the [HotStuff-2](https://eprint.iacr.org/2023/397.pdf) protocol. 
* We never emit a `QuorumProposalValidated` event if we are missing any data.  It is unsafe to emit this event if we can't properly validate the entire proposal. 
* For simplicity, we opt to have nodes not reprocess proposals once they have the data they were missing.  Instead they will wait for the next proposal. In the future we can change this behavior so that nodes finish processing their current proposal once they have the data they are missing. 

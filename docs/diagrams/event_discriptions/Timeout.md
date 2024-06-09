# Timeout

![Timeout](/docs/diagrams/images/HotShotFlow-Timeout.drawio.png "Timeout")

* The Timeout event signals that the node has not seen evidence for the view referenced in the event within the expected time window.  For example: if a node is in view 10 and does not receive evidence to change to view 11 within the expected time window, a `Timeout(11, _)` event will be created.  
* `evidenced` is a boolean value indicating whether the previous view had view change evidence.  Using our example above, `Timeout(11, true)` means that the node saw valid evidence for view 10. This parameter is used to decide whether a node should send a timeout vote or trigger view sync.  A timeout vote is sent upon the first view timeout after a successful view.  View sync, on the other hand, is triggered on the second view timeout after a successful view.  
* It is important to update our `latest_voted_view` variable to indicate that the node should no longer vote for the previous view.  This is to prevent conflicting certificates from forming in the case that nodes timeout, but receive a late `QuorumProposal` after the timeout.  For safety, only one view change evidence certificate can be formed per view. 

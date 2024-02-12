# ViewChange

![ViewChange](/docs/diagrams/images/HotShotFlow-ViewChange.drawio.png "ViewChange")

* This task updates the `latest_known_view` a node is aware of.  It also handles canceling any in-progress tasks from previous views (such as timeout tasks waiting to publish a timeout event)
* In the optimistic case, a leader will request their block from the builder 1 view ahead of their leader slot.  However, it's possible to skip views due to a node catching up after being offline or the network participating in view sync.  In this case, we allow the leader to request their block during their leader view. 
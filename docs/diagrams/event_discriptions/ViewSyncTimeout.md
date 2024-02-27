# ViewSyncTimeout

![ViewSyncTimeout](/docs/diagrams/images/HotShotFlow-ViewSyncTimeout.drawio.png "ViewSyncTimeout")

* The `ViewSyncTimeout` event is similar to the `Timeout` event, except that the former is used exclusively in the context of view sync. 
* When a view sync round times out a node will increment its `latest_known_relay` value by 1, and then resend its latest view sync vote. 
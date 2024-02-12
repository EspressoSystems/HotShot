# VIDDataFromBuilderRecv

![VIDDataFromBuilderRecv](/docs/diagrams/images/HotShotFlow-VIDDataFromBuilderRecv.drawio.png "VIDDataFromBuilderRecv")

* This diagram assumes we trust the builder to give our node a valid block payload. In the future we can specify logic if we do not trust the builder (and therefore need some fair exchange of signatures in place)
* The builder sends its data in two parts: the block payload and vid commitment, since the vid commitment can be costly for the builder to calculate.  This task handles the latter task. 
* In the current PBS design (which will become more holistic in the future) the builder is expected to calculate the vid commitment on the node's behalf.  The node uses this commitment to then calculate the vid shares. 
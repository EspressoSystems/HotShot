# BlockFromBuilderRecv

![BlockFromBuilderRecv](/docs/diagrams/images/HotShotFlow-BlockFromBuilderRecv.drawio.png "BlockFromBuilderRecv")

* This diagram assumes we trust the builder to give our node a valid block payload. In the future we can specify logic if we do not trust the builder (and therefore need some fair exchange of signatures in place)
* The builder sends its data in two parts: the block payload and vid commitment, since the vid commitment can be costly for the builder to calculate.  This task handles the former. 
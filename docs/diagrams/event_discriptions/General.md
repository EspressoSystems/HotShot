## General Notes
* "Valid" certificates meet the following criteria: 
  * Aggegated signature is valid against the data the certificate commits to
  * Aggregated signature represents a proper threshold for that certificate

## Data Structures
```
state_map {
  HashMap (VIDCommitment -> Leaf)
  TODO
}
```

```
latest_known_view: u64
```

```
latest_voted_view: u64
```

```
latest_da_voted_view: u64
```
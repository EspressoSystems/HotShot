<a name="unreleased"></a>
## [Unreleased]


<a name="0.0.3"></a>
## [0.0.3] - 2021-10-27
### Features
- Implement janky catchup

### BREAKING CHANGE

Adds new type paramaeter, corrosponding to the state type, to Message

<a name="0.0.2"></a>
## [0.0.2] - 2021-10-19
### Bug Fixes
- Fix leaders not sending themselves commit votes
- Fix state not getting stored properly

### Features
- StatefulHandler trait
- Reexport traits from traits module
- State Machine + NodeImplementation
- state machine mvp megasquash
- Replace tokio broadcast queue with unbounded equivalent

### BREAKING CHANGE

Changes queue type in phaselock methods


<a name="0.0.1"></a>
## [0.0.1] - 2021-08-20

<a name="0.0.0"></a>
## 0.0.0 - 2021-07-07

[Unreleased]: https://gitlab.com/translucence/systems/phaselock/compare/0.0.3...HEAD
[0.0.3]: https://gitlab.com/translucence/systems/phaselock/compare/0.0.2...0.0.3
[0.0.2]: https://gitlab.com/translucence/systems/phaselock/compare/0.0.1...0.0.2
[0.0.1]: https://gitlab.com/translucence/systems/phaselock/compare/0.0.0...0.0.1

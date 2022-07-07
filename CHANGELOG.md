<a name="0.0.4"></a>
## [0.0.4] - 2021-11-01
### Features
- Downgrade possibly-temporary network faults to warnings
- Improve logging when an invalid transaction is submitted


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

Changes queue type in hotshot methods


<a name="0.0.1"></a>
## [0.0.1] - 2021-08-20

<a name="0.0.0"></a>
## 0.0.0 - 2021-07-07

[Unreleased]: https://github.com/EspressoSystems/hotshot/compare/0.0.4...HEAD
[0.0.4]: https://github.com/EspressoSystems/hotshot/compare/0.0.3...0.0.4
[0.0.3]: https://github.com/EspressoSystems/hotshot/compare/0.0.2...0.0.3
[0.0.2]: https://github.com/EspressoSystems/hotshot/compare/0.0.1...0.0.2
[0.0.1]: https://github.com/EspressoSystems/hotshot/compare/0.0.0...0.0.1

# Purpose

Infrastructure and integration tests for hotshot.

# Usage

The overall control flow is:

```
TestBuilder::default().build() ->  TestLauncher::launch()      -> TestRunner::execute()
|                                  |                              |
- easy override setup fn           - more explicit overrides      - executes the test
|                                    | for networking, storage,
- easy override correctness fn       | hooks/overrides etc
|
- easily add in hooks
|
- easily override launching
```

Easily overriding setup/correctness checks/hooks and launching is all done by anonymous functions. Fairly sane and configurable setup and correct check functions may be generated from  TODO.

See TODO for examples.

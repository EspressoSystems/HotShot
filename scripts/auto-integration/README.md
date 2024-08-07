# Auto-Integration

To use this set up a virtualenv as
```shell
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
```

```
Help settings
 Usage: run-integration.py [OPTIONS]

 Watches the native demo and reports any HotShot failures.

╭─ Options ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ --view-threshold            TEXT  [default: 10000]                                                                                                  │
│ --host-ip                   TEXT  [default: localhost]                                                                                              │
│ --install-completion              Install completion for the current shell.                                                                         │
│ --show-completion                 Show completion for the current shell, to copy it or customize the installation.                                  │
│ --help                            Show this message and exit.                                                                                       │
╰─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

View threshold is the number of views that this will run for.

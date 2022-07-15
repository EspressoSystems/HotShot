# WIP Specification for HotShot Protocol

[Link to spec document](./src/HotShot.md)

## How to build

### With Nix (+ flakes)

In the root directory:

```shell
nix build . -o hotshot-spec.pdf
```

### With Nix (no flakes)

In the `src` directory, run:

``` shell
nix-shell --run "make"
```

You can then run the following to clean up excess artifacts:

``` shell
nix-shell --run "make clean"
```


### Without Nix

Make sure you have texlive installed with at least the following packages:
- `scheme-small`
- `multirow`
- `xstring`
- `totpages`
- `environ`
- `ncctools`
- `comment`
- `hyperxmp`
- `ifmtarg`
- `preprint`
- `latexmk`
- `libertine`
- `inconsolata`
- `shade`
- `newtx`

Additionally, [install Pandoc](https://pandoc.org/installing.html) if needed

Once all dependencies are installed, inside the `src` directory:

```shell
make
```

# TODO

- [ ] Translate psuedocode to algorithim2e
- [ ] Incorporate analysis section

# Eternalist Foundry

Foundry turns a product-owned evidence contract into native CI jobs, verifies
that every planned proof returned a receipt, and emits the exact support
manifest from which Eternalist's public platform claims are derived.

It does not own application tests or package construction. Applications expose
independently runnable proof commands. [`dist`](https://github.com/axodotdev/cargo-dist)
builds CLI distributions; Cargo Packager builds native GUI containers. Foundry
schedules those engines, records their products, and refuses publication when
the declared support envelope and the surviving evidence differ.

The initial profiles are native GUI, portable CLI, Rust library, and explicitly
platform-bound Rust products. Baseline coordinates are closed values rather
than arbitrary runner labels.

```console
foundry check
foundry plan
foundry prove source
foundry judge evidence/
foundry support evidence/ support.json
```

`foundry.toml` is the repository boundary. GitHub Actions is only one scheduler
over the same commands.

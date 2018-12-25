# taildir

`tail -f` for directory, recursively.

## Feature

- cross platform & debounce event (via `notify`)
- file filter
- line filter

## Install

```toml
[dependencies]
walkdir = "0.2"
```

## Exmaple

> cargo run bin --example
>
> ./test/append.sh
>
> (check terminal output)

for detail, see [src/bin/example.rs](src/bin/example.rs).
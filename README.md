
# nova_fuzz

A harness for running [Fuzzilli](https://github.com/googleprojectzero/fuzzilli) against [Nova](https://github.com/trynova/nova).

## Building

For the fuzzer to work effectively we need instrumentation to be built into all dependencies. We also need to link in Fuzzilli's implementation of this instrumentation as apposed to using libFuzzer or AFL.

```sh
gcc -c src/coverage.c -o coverage.o
export RUSTFLAGS="-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=1 -C llvm-args=-sanitizer-coverage-trace-pc-guard -C link-arg=$(pwd)/coverage.o"
cargo build
```

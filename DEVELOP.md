```bash
cargo run --release -- -h
cargo run --release -- \
    tests/example_cmake_project \
    src/main.cpp \
    build/main \
    --in-place-template \
    --data '[{"N": 10}, {"N": 20}]'
```
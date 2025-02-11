jq -n --argjson n 50 '[{"N": "a"}] + [range(1; $n + 1) | {N: .}]' > datas.json
# cargo build --release
./target/release/parabuild tests/example_run_time_consuming_project build/main --template-file src/main.cpp --data-file datas.json -j 2 -J 2 --output-file output.json --autosave 1s > /dev/null 2>&1 &
PID=$!
sleep 10
kill -SIGKILL $PID
wait $PID
./target/release/parabuild tests/example_run_time_consuming_project build/main --template-file src/main.cpp --data-file datas.json -j 2 -J 2 --output-file output.json --continue > /dev/null 2>&1
run_len=$(jq length output.json)
compile_error_len=$(jq length compile_error_datas.json)
sum=$(($run_len + $compile_error_len))
if [ $sum -eq 51 ]; then
    echo "pass"
else
    echo "fail, $sum != 51"
    exit 1
fi
rm -rf .parabuild output.json compile_error_datas.json datas.json
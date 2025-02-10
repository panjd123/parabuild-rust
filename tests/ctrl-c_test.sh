jq -n --argjson n 100 '[{"N": "a"}] + [range(1; $n + 1) | {N: .}]' > datas.json
./target/release/parabuild tests/example_run_time_consuming_project build/main --template-file src/main.cpp --data-file datas.json -j 2 -J 2 --output-file output.json > /dev/null 2>&1 &
PID=$!
sleep 10
kill -SIGINT $PID
wait $PID
unprocessed_len=$(jq length "$(ls -d .parabuild/autosave/*/ | sort -r | head -n 1)unprocessed_data.json")
run_len=$(jq length output.json)
compile_error_len=$(jq length compile_error_datas.json)
sum=$(($unprocessed_len + $run_len + $compile_error_len))
if [ $sum -eq 101 ]; then
    echo "pass"
else
    echo "fail, $sum != 101"
    exit 1
fi
rm -rf .parabuild output.json compile_error_datas.json datas.json
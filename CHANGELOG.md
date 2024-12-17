# 0.2.9

- Fix summary output when all data compiled failed
- Optimize error prompts
- Add spinner when running in-place
- Run `cargo update`

# 0.2.5 - 0.2.8

- Support Makefile project by passing `CPPFLAGS`, check documentation for more details
- Support more cli options like `--panic-on-compile-error` and `format-output`
- Refactoring cli parameter logic for `--run-in-place`

# 0.2.4

- Use rsync by default
- Modify some default parameters

# 0.2.3

- Now use `target_files` to represent general compilation products which should be migrated from `build` to `run` workspace
- Add environment variable `PARABUILD_ID` to represent the unique id of the current run worker
- Optimize the execution process to prevent `text file is busy` error, please install `lsof` to use this feature
- Add MIG support, auto set environment variables `CUDA_VISIBLE_DEVICES` when MIG is enabled in your system

# 0.2.2

- Add handlebars helper: `{{default name 'default_value'}}`
- Optimize the progress bar when init_workspace
- Modify some default parameters
- Add runtime output

# 0.2.1

- Minor improvement in cp performance under in-place template conditions

# 0.2.0

- Support in-place template rendering
- Support progress bar
- Fix exclusive run
- Add a formal command-line application

# 0.1.0

- Initial release
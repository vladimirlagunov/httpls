SHELL := /bin/bash -e -o pipefail

export DYLD_LIBRARY_PATH:=/Users/lvo/rust-git/lib
export rustc:=/Users/lvo/rust-git/bin/rustc -L . -g


main: main.rs libhttp_server2.rlib
	$(rustc) $< -o $@

.PHONY: clean
clean:
	rm -f lib*.rlib main


lib%.rlib: %.rs
	$(rustc) $<


%: %.rs
	$(rustc) $<

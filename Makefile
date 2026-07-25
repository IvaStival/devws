.PHONY: install install-deps check

install:
	@./install.sh

install-deps:
	@./install.sh --deps

check:
	@./scripts/check.sh

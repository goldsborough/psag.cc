#!/bin/bash
export RUST_BACKTRACE=1
export RUST_LOG='psag_cc=debug'
export HOST_PORT='127.0.0.1:3000'
export DATABASE_URL="postgresql://$(whoami)@localhost:5432"
export WWW_DIR='static/www'

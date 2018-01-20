#!/bin/bash
export RUST_BACKTRACE=1
export RUST_LOG='url_shortener=debug'
export HOST_PORT='127.0.0.1:3000'
export DATABASE_URL='postgresql://goldsborough@localhost:5432'
export WWW_DIR='static/www'

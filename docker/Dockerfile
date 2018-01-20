FROM rust
MAINTAINER <peter@goldsborough.me>

# Install vim for in-place editing.
RUN apt-get update -y && apt-get install -y vim

WORKDIR /var/www/psag.cc/
COPY . .

RUN cargo install

CMD ["psag_cc"]

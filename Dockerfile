FROM rust

WORKDIR /var/www/psag.cc/
COPY . .

RUN cargo install

CMD ["url-shortener"]

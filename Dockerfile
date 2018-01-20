FROM rust

WORKDIR /usr/src/url-shortener
COPY . .

RUN cargo install

CMD ["url-shortener"]

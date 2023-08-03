# NOTE run from parent directory
FROM ubuntu:jammy

COPY target/release-lto/examples/web-server /bin/web-server

EXPOSE 9000

CMD ["web-server"]

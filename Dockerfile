FROM debian:bullseye-slim
EXPOSE 3333

COPY target/release/blitz-website /usr/local/bin/blitz-website
RUN chmod +x /usr/local/bin/blitz-website

WORKDIR /usr/blitz-website
COPY ./static ./static
COPY ./data ./data


CMD ["blitz-website"]
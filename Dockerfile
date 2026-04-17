FROM ubuntu:24.04
EXPOSE 3333

# Update certificate store
RUN apt-get update && apt-get install -y ca-certificates && update-ca-certificates

COPY target/release/blitz-website /usr/local/bin/blitz-website
RUN chmod +x /usr/local/bin/blitz-website

WORKDIR /usr/blitz-website
COPY ./static ./static
COPY ./data ./data


CMD ["blitz-website"]
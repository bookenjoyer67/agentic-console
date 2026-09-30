# Credential broker for the Komun sandbox.
#
# The agent container holds no provider credentials at all: it runs on an
# --internal docker network and talks only to this broker, which injects the
# real keys upstream. Runs as uid 1000 so files it writes back (refreshed OAuth
# credentials) stay owned by the human user on the host.
FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

WORKDIR /broker
COPY broker.py /broker/broker.py

# No root need: the code only reads the mounted secrets and streams HTTP.
USER 1000:1000

EXPOSE 4000
CMD ["python3", "/broker/broker.py"]

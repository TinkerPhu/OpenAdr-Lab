# Pinned by digest, not by the moving `3.12-slim` tag.
#
# A digest makes a re-pull byte-identical, which is what keeps every layer below
# cacheable. With the bare tag, any host that loses the base image — a
# `docker system prune` to reclaim disk is enough, and the base is only ever a
# build-time image so prune always considers it unused — re-pulls whatever
# upstream publishes today. A new image ID then invalidates the apt layer and
# everything after it, including the 115 MB Chromium download below. That is
# exactly what was happening on the secondary docker host: the test-runner image
# was three days old, `python:3.12-slim` was gone, and every E2E run paid ~5
# minutes to rebuild a layer whose inputs had not changed.
#
# The trade-off this accepts: base-image security updates no longer arrive
# implicitly, so bump this deliberately (`docker buildx imagetools inspect
# python:3.12-slim` prints the current index digest). Pin the multi-arch index
# digest, never a per-platform one — the docker hosts here are arm64 while local
# WSL is x86_64, and a single-platform digest would break one of them.
FROM python:3.12-slim@sha256:dddfd7e07f9d15aeeca61529320492139d21cac7f0070c00609243e51e4e0016

RUN apt-get update && apt-get install -y --no-install-recommends \
    postgresql-client curl ca-certificates gnupg \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian $(. /etc/os-release && echo $VERSION_CODENAME) stable" > /etc/apt/sources.list.d/docker.list \
    && apt-get update && apt-get install -y --no-install-recommends docker-ce-cli docker-compose-plugin \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tests

# The browser download gets its own layer, ahead of the other test dependencies:
# it is by far the most expensive step here (115 MB), and it has no reason to
# repeat when an unrelated dependency like `behave` or `requests` changes.
# `requirements.txt` includes this same file, so the version is declared once.
COPY requirements-browser.txt .
RUN pip install --no-cache-dir -r requirements-browser.txt \
    && playwright install chromium --with-deps

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY behave.ini .
COPY features/ features/
COPY provision_ven.py .
COPY entrypoint.sh .
RUN sed -i 's/\r$//' entrypoint.sh && chmod +x entrypoint.sh

ENTRYPOINT ["./entrypoint.sh"]

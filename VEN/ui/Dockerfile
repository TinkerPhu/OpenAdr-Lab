# The build context is the repo root: this app is one workspace of an npm
# workspace whose dependencies hoist to the root node_modules, and it imports
# the shared chart source at ui-charts/. A context of VEN/ui would see neither.
FROM node:20-alpine AS build
WORKDIR /repo

# Manifests first, so a source-only change reuses the install layer. Every
# workspace's package.json is needed: npm resolves the whole workspace graph
# before it installs any of it.
COPY package.json package-lock.json ./
COPY VEN/ui/package.json VEN/ui/
COPY VTN/ui/package.json VTN/ui/
RUN npm ci

COPY ui-charts/ ui-charts/
COPY VEN/ui/ VEN/ui/

# Allow build-time VITE_ env vars (e.g. for test stack pointing at test containers)
ARG VITE_VEN_1_URL
ARG VITE_VEN_2_URL
ARG VITE_VEN_3_URL
RUN npm run build --workspace VEN/ui

FROM nginx:alpine
COPY VEN/ui/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /repo/VEN/ui/dist /usr/share/nginx/html
EXPOSE 80

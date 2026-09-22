# The build context is the repo root: this app is one workspace of an npm
# workspace whose dependencies hoist to the root node_modules, and it imports
# the shared chart source at ui-charts/. A context of VTN/ui would see neither.
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
COPY VTN/ui/ VTN/ui/
RUN npm run build --workspace VTN/ui

FROM nginx:alpine
COPY VTN/ui/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /repo/VTN/ui/dist /usr/share/nginx/html
EXPOSE 80

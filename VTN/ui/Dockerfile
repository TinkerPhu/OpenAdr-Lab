# The build context is the repo root: this app is one workspace of an npm
# workspace whose dependencies hoist to the root node_modules, and it imports
# the shared UI source at lab-ui/. A context of VTN/ui would see neither.
FROM node:20-alpine AS build
WORKDIR /repo

# Manifests first, so a source-only change reuses the install layer. Every
# workspace's package.json is needed: npm resolves the whole workspace graph
# before it installs any of it.
COPY package.json package-lock.json ./
COPY VEN/ui/package.json VEN/ui/
COPY VTN/ui/package.json VTN/ui/
# npm's download cache survives in a BuildKit cache mount, so a lockfile change
# fetches only the packages that changed.
RUN --mount=type=cache,id=npm-cache,target=/root/.npm npm ci --prefer-offline --no-audit --no-fund

COPY lab-ui/ lab-ui/
COPY VTN/ui/ VTN/ui/
RUN npm run build --workspace VTN/ui

FROM nginx:alpine
COPY VTN/ui/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /repo/VTN/ui/dist /usr/share/nginx/html
EXPOSE 80

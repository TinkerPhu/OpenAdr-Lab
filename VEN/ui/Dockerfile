FROM node:20-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
# Allow build-time VITE_ env vars (e.g. for test stack pointing at test containers)
ARG VITE_VEN_1_URL
ARG VITE_VEN_2_URL
ARG VITE_VEN_3_URL
RUN npm run build

FROM nginx:alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80

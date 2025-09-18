# syntax=docker/dockerfile:1

FROM node:24-alpine
# install dependencies and build TS → JS
WORKDIR /app
COPY package.json package-lock.json tsconfig.json ./
COPY src ./src/
COPY test ./test/
COPY cache ./cache
COPY node_modules ./node_modules
COPY dist ./dist
RUN npm --version
RUN npm run build
RUN echo "✅ Build completed"
CMD ["npm", "run", "agent"]
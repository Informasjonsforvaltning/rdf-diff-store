# rdf-query-cache

```
docker run --rm -v "${PWD}:/local" openapitools/openapi-generator-cli sh -c "/usr/local/bin/docker-entrypoint.sh generate -i /local/openapi.yaml -g rust -o /out && rm -rf /local/src/models && mv /out/src/models /local/src/models && chown -R $(id -u):$(id -g) /local/src/models"
```

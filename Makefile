models:
	docker run --rm -v "${PWD}:/local" openapitools/openapi-generator-cli sh -c "/usr/local/bin/docker-entrypoint.sh generate -i /local/openapi.yaml -g rust -o /out && rm -rf /local/src/models && mv /out/src/models /local/src/models && chown -R $(id -u):$(id -g) /local/src/models"

test: dockerup rusttest dockerdown

rusttest:
	rm -rf ./tmp-repos/test
	GIT_REPOS_ROOT_PATH=./tmp-repos/test \
	GIT_REPO_URL=http://gitea:gitea123@localhost:3000/gitea/diff-store.git \
		cargo test -- --test-threads 1

runwriter:
	RDF_PRETTIFIER_URL=https://rdf-prettifier.staging.fellesdatakatalog.digdir.no/api/prettify \
	RDF_PRETTIFIER_API_KEY="${RDF_PRETTIFIER_API_KEY}" \
	GIT_REPOS_ROOT_PATH=./tmp-repos/writer \
	GIT_REPO_URL=http://gitea:gitea123@localhost:3000/gitea/diff-store.git \
		cargo run --bin rdf-diff-writer

runquery:
	RDF_PRETTIFIER_URL=https://rdf-prettifier.staging.fellesdatakatalog.digdir.no/api/prettify \
	RDF_PRETTIFIER_API_KEY="${RDF_PRETTIFIER_API_KEY}" \
	GIT_REPOS_ROOT_PATH=./tmp-repos/query \
	GIT_REPO_URL=http://gitea:gitea123@localhost:3000/gitea/diff-store.git \
		cargo run --bin rdf-query-cache

dockerup:
	docker-compose up -d

dockerdown:
	docker-compose down -v

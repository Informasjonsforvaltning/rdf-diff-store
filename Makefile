test: dockerup rusttest dockerdown
run: dockerup rustrun dockerdown

rusttest:
	rm -rf ./tmp-repos
	GIT_REPOS_ROOT_PATH=./tmp-repos \
	GIT_REPO_URL=http://gitea:gitea123@localhost:3000/gitea/diff-store.git \
		cargo test -- --test-threads 1

rustrun:
	GIT_REPOS_ROOT_PATH=./tmp-repos \
	GIT_REPO_URL=http://gitea:gitea123@localhost:3000/gitea/diff-store.git \
		cargo run

dockerup:
	docker-compose up -d

dockerdown:
	docker-compose down -v

test:
	docker-compose up -d
	GIT_URL=http://localhost:3000/gitea/diff-store.git \
		cargo test -- --test-threads 1
	docker-compose down -v

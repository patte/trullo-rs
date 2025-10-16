# trullo 

```
project/
├─ assets/ # Any assets that are used by the app should be placed here
├─ src/
│  ├─ main.rs # main.rs is the entry point to your application and currently contains all components for the app
├─ Cargo.toml # The Cargo.toml file defines the dependencies and feature flags for your project
```

### Tailwind
1. Install npm: https://docs.npmjs.com/downloading-and-installing-node-js-and-npm
2. Install the Tailwind CSS CLI: https://tailwindcss.com/docs/installation
3. Run the following command in the root of the project to start the Tailwind CSS compiler:

```bash
npx tailwindcss -i ./tailwind.css -o ./assets/tailwind.css --watch
```

### Serving Your App

Run the following command in the root of your project to start developing with the default platform:

```bash
dx serve --platform web
```

## Commands (server feature)

This project also exposes a couple of CLI commands when built with the `server` feature. These commands interact with a local SQLite database and, for `import-sms`, a MikroTik router.

### Build with server feature

```bash
cargo build --no-default-features --features server
```

The compiled binary will be at `target/debug/trullo-rs`.

### Database location

By default (when `DATABASE_URL` isn’t set), the app stores data in `data/data.db` under the project root. The URL used internally is of the form `sqlite:///abs/path/to/data.db?mode=rwc`. The `data/` directory is created automatically if it doesn’t exist.

You can override the database location by setting `DATABASE_URL` (e.g., `sqlite:///absolute/path/to/your.db?mode=rwc`) in your environment or a `.env` file.

### Environment variables for MikroTik (import-sms)

`import-sms` needs access to your MikroTik’s REST API. Configure one of the following auth methods via environment variables (a `.env` file is supported):

- Required:
	- `MIKROTIK_URL` (e.g., `http://192.168.88.1`)
- Authentication (choose one):
	- `MIKROTIK_AUTH_BASE64` (contents of `base64(username:password)`)
	- or `MIKROTIK_USER` and `MIKROTIK_PASSWORD` (or `MIKROTIK_PASS`)

Example `.env`:

```env
MIKROTIK_URL=http://192.168.88.1
MIKROTIK_USER=admin
MIKROTIK_PASSWORD=yourpassword
# DATABASE_URL=sqlite:///absolute/path/to/data.db?mode=rwc
```

### Commands

- `gen-test-data [PLAN_TOTAL_MB]`
	- Generates ~90 days of synthetic readings to the SQLite DB.
	- `PLAN_TOTAL_MB` is optional (default: `102400` which is ~100 GB).
	- Example:
		```bash
		target/debug/trullo-rs gen-test-data 204800
		```

- `import-sms`
	- Fetches all SMS from the MikroTik inbox, parses WindTre data status messages, and inserts them into the DB.
	- Duplicate records are ignored (uniqueness by timestamp).
	- Example:
		```bash
		# Ensure .env contains MikroTik and optional DATABASE_URL
		target/debug/trullo-rs import-sms
		```

Tips:
- Both commands respect a `.env` file in the project root (via `dotenvy`).
- Run with `RUST_LOG` or check stderr for progress messages.

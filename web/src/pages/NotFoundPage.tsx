import { Link } from "react-router";
import { SearchBox } from "../components/SearchBox";

/** Unknown paths and anything that isn't an IP address (the server sends it with a 404). */
export function NotFoundPage() {
  return (
    <>
      <title>Not found · iplookup</title>
      <section className="card message-card">
        <h1>Page not found</h1>
        <p className="muted">
          There's no page here, and it isn't an IP address either. Look one up instead:
        </p>
        <SearchBox />
        <p>
          <Link to="/">See your own IP address</Link>
        </p>
      </section>
    </>
  );
}

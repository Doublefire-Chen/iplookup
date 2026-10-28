import { README_URL, VERSION_PATH } from "../links";

/** Credits every Data Source (and the map tiles), as their licences require. */
export function Footer() {
  return (
    <footer className="footer">
      <p>
        This product includes GeoLite Data created by MaxMind, available from{" "}
        <a href="https://www.maxmind.com">maxmind.com</a>.
      </p>
      <p>
        IP address data powered by <a href="https://ipinfo.io">IPinfo</a>. Map data ©{" "}
        <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors.
      </p>
      <nav className="footer-links" aria-label="About">
        <a href={README_URL}>API docs</a>
        {/* A full page load: /version is answered by the server. */}
        <a href={VERSION_PATH}>Version</a>
      </nav>
    </footer>
  );
}

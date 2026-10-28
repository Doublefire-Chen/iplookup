import { Link, Outlet } from "react-router";
import { Footer } from "./Footer";

export function Layout() {
  return (
    <div className="shell">
      <header className="topbar">
        <Link to="/" className="brand">
          iplookup
        </Link>
      </header>
      <main className="content">
        <Outlet />
      </main>
      <Footer />
    </div>
  );
}

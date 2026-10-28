import { lazy, Suspense } from "react";
import type { MapCircle } from "../map";

// Leaflet (and its CSS) is a separate chunk under /assets/, fetched only
// once a page has something to draw, so it never delays the Record itself.
const LeafletMap = lazy(() => import("./LeafletMap"));

/**
 * A map of where an address probably is: a circle of the Accuracy Radius
 * around the Record's coordinates, deliberately with no pin. Renders
 * nothing at all when there is no circle to draw.
 */
export function AccuracyMap({ circles }: { circles: MapCircle[] }) {
  if (circles.length === 0) return null;
  return (
    <section className="card map-card" aria-label="Map">
      <Suspense fallback={<div className="map" />}>
        <LeafletMap circles={circles} />
      </Suspense>
      <p className="map-note muted">
        {circles.length === 1 ? "The address is" : "Each address is"} somewhere in{" "}
        {circles.length === 1 ? "the circle" : "its circle"}; the coordinates are only a rough
        centre.
      </p>
    </section>
  );
}

import "leaflet/dist/leaflet.css";
import { latLng, latLngBounds, type LatLngBounds } from "leaflet";
import { useEffect, useMemo } from "react";
import { Circle, MapContainer, TileLayer, useMap } from "react-leaflet";
import type { MapCircle } from "../map";

const TILES = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
const ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors';
const PADDING: [number, number] = [12, 12];

/** The Leaflet map behind AccuracyMap; loaded lazily (default export for `lazy`). */
export default function LeafletMap({ circles }: { circles: MapCircle[] }) {
  const bounds = useMemo(() => boundsOf(circles), [circles]);
  return (
    <MapContainer
      className="map"
      bounds={bounds}
      boundsOptions={{ padding: PADDING }}
      scrollWheelZoom={false}
      // Fractional zoom lets the circle fill the map instead of snapping to
      // the next whole zoom level out.
      zoomSnap={0.25}
      worldCopyJump
    >
      <TileLayer url={TILES} attribution={ATTRIBUTION} maxZoom={19} />
      {circles.map((c) => (
        <Circle
          key={`${c.center.lat},${c.center.lon},${c.radiusMeters}`}
          center={[c.center.lat, c.center.lon]}
          radius={c.radiusMeters}
          // Leaflet reads className only when it creates the path, so it
          // can't go in pathOptions. Its colours come from styles.css.
          className="accuracy-circle"
          weight={2}
          fillOpacity={0.15}
        />
      ))}
      <FitBounds bounds={bounds} />
    </MapContainer>
  );
}

/** Refits the zoom when the circles change (a Self Lookup card finishing late). */
function FitBounds({ bounds }: { bounds: LatLngBounds }) {
  const map = useMap();
  useEffect(() => {
    map.fitBounds(bounds, { padding: PADDING });
  }, [map, bounds]);
  return null;
}

/** The smallest box holding every circle. */
function boundsOf(circles: MapCircle[]): LatLngBounds {
  const boxes = circles.map((c) =>
    latLng(c.center.lat, c.center.lon).toBounds(c.radiusMeters * 2),
  );
  const all = latLngBounds(boxes[0].getSouthWest(), boxes[0].getNorthEast());
  for (const box of boxes.slice(1)) all.extend(box);
  return all;
}

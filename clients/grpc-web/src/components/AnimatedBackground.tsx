"use client";

export function AnimatedBackground() {
  return (
    <>
      {/* Base radial gradient layer with hue cycling */}
      <div className="fixed inset-0 z-[-3] bg-radial-wallet animate-hue-cycle" />

      {/* Rotating conic gradient overlay */}
      <div className="fixed inset-0 z-[-2] bg-conic-wallet animate-rotate-bg" />

      {/* Noise texture layer */}
      <div
        className="pointer-events-none fixed inset-0 z-[-1] opacity-[var(--noise-opacity)]"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.65' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E")`,
          backgroundSize: "160px 160px",
        }}
      />

      {/* Vignette effect */}
      <div className="pointer-events-none fixed inset-0 z-[-1] bg-vignette" />
    </>
  );
}

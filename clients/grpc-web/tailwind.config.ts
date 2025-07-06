import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        // primaries
        "brand-pink": "#E551FF",
        "brand-purple": "#7A46FF",
        "brand-blue": "#4C6FFF",
        "brand-teal": "#00FFA3",
        // neutrals
        "surface-900": "#0F172A", // card bg
        "surface-800": "#16203B", // elevated bg
        "surface-700": "#1D2949", // nav bg
      },
      backgroundImage: {
        "x-gradient": "linear-gradient(135deg,#182848 0%,#1F1A3C 100%)",
      },
    },
  },
  plugins: [],
};

export default config;

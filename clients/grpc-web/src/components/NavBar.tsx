interface NavBarProps {
  isLoading: boolean;
  grpc: boolean;
  onSendEvent: () => void;
}

export function NavBar({ isLoading, grpc, onSendEvent }: NavBarProps) {
  return (
    <header className="glass-card flex items-center justify-between px-4 py-2 fixed top-4 left-1/2 -translate-x-1/2 w-[calc(100%-5rem)] max-w-8xl z-40">
      <h1 className="text-lg font-semibold text-white flex items-center gap-2">
        {/* Logo placeholder */}
        <span
          className="h-3 w-3 rounded-full animate-pulse-success"
          style={{ backgroundColor: "#E551FF" }}
        />
        Silvana RPC Demo
      </h1>

      <div className="flex items-center gap-4 text-sm text-white/70">
        {/* Live status – will be toggled below */}
        <span className="flex items-center gap-1">
          <span
            id="grpc-dot"
            className="h-2 w-2 rounded-full"
            style={{ backgroundColor: "rgba(0, 255, 163, 0.6)" }}
          />
          RPC
        </span>
        <span className="flex items-center gap-1">
          <span
            id="nats-dot"
            className="h-2 w-2 rounded-full"
            style={{ backgroundColor: "rgba(0, 255, 163, 0.6)" }}
          />
          NATS
        </span>

        {/* Send Event Button */}
        <button
          className="modern-button text-white font-semibold text-base w-40 h-10 flex items-center justify-center"
          onClick={isLoading || !grpc ? undefined : onSendEvent}
        >
          {isLoading ? (
            <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></div>
          ) : (
            <span>Send Event</span>
          )}
        </button>
      </div>
    </header>
  );
}

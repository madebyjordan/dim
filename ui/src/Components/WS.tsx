import { createContext, useEffect, useRef, useState } from "react";

import { useAppSelector } from "../hooks/store";
import { DimWebSocket, type ConnectionState } from "../api/websocket";
import DimLogo from "../assets/DimLogo";
import Bar from "../Components/Load/Bar";

import "./WS.scss";

export const WebSocketContext = createContext<WebSocket | null>(null);

function WS({ children }: React.PropsWithChildren) {
  const token = useAppSelector((state) => state.auth.token);
  const [socket, setSocket] = useState<WebSocket | null>(null);
  const [state, setState] = useState<ConnectionState>("connecting");
  const [retryInMs, setRetryInMs] = useState<number>();
  const manager = useRef<DimWebSocket | undefined>(undefined);

  useEffect(() => {
    const connection = new DimWebSocket({
      onSocket: setSocket,
      onState: (next, retry) => {
        setState(next);
        setRetryInMs(retry);
      },
    });
    manager.current = connection;
    connection.start(null);
    return () => connection.stop();
  }, []);

  useEffect(() => manager.current?.setToken(token), [token]);

  const initialFailure = !socket && ["offline", "reconnecting"].includes(state);
  if (initialFailure) {
    return (
      <div className="appLoad showAfter100ms">
        <DimLogo />
        <div className="error">
          <h2>
            {state === "offline" ? "Server unavailable" : "Connection lost"}
          </h2>
          <p>Dim will keep trying with a bounded backoff.</p>
          <button onClick={() => manager.current?.retryNow()}>
            Reconnect now{retryInMs ? ` (${Math.ceil(retryInMs / 1000)}s)` : ""}
          </button>
        </div>
      </div>
    );
  }

  if (!socket && state === "connecting") {
    return (
      <div className="appLoad showAfter100ms">
        <DimLogo />
        <h2>Connecting to server</h2>
        <Bar />
      </div>
    );
  }

  return (
    <WebSocketContext.Provider value={socket}>
      {children}
    </WebSocketContext.Provider>
  );
}

export default WS;

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";

import App from "./App";
import { store } from "./store";

const app = (
  <StrictMode>
    <Provider store={store}>
      <App />
    </Provider>
  </StrictMode>
);

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Unable to find the application root element.");
}

createRoot(rootElement).render(app);

import ReactDOM from "react-dom/client";
import App from "./App";

// Note: no StrictMode — its double-mounted effects would open and
// immediately tear down every SSH session in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);

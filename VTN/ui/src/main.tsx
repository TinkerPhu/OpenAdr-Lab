import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createTheme, ThemeProvider, CssBaseline } from "@mui/material";
import App from "./App";

const theme = createTheme({
  palette: {
    primary: { main: "#00695c" },   // teal 800
    secondary: { main: "#ff8f00" }, // amber 800
  },
});

console.log("[VTN-UI] main.tsx executing at", new Date().toISOString());

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 5_000,
    },
  },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </ThemeProvider>
  </React.StrictMode>,
);

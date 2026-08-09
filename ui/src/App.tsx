import { BrowserRouter, Route, Routes } from "react-router";

import WS from "./Components/WS";

import ThemeController from "./Controllers/Theme";
import FaviconController from "./Controllers/Favicon";

import NotAuthedOnlyRoute from "./Routes/NotAuthedOnly";
import PrivateRoute from "./Routes/Private";
import {
  AuthSessionController,
  CrossTabAuthController,
  ScrollToTop,
} from "./Routes/SessionControllers";
import MainLayout from "./Layouts/MainLayout";
import Notifications from "./Components/Notifications";

import Dashboard from "./Pages/Dashboard/Index";
import Library from "./Pages/Library/Index";
import Media from "./Pages/Media/Index";
import VideoPlayer from "./Pages/VideoPlayer/Index";
import SearchResults from "./Pages/SearchResults/Index";
import Login from "./Pages/Auth/Login";
import Register from "./Pages/Auth/Register";
import Preferences from "./Pages/Preferences/Index";

import "./App.scss";

const ApplicationRoutes = () => (
  <Routes>
    <Route element={<NotAuthedOnlyRoute />}>
      <Route path="/login" element={<Login />} />
      <Route path="/register" element={<Register />} />
    </Route>

    <Route element={<PrivateRoute />}>
      <Route element={<MainLayout />}>
        <Route index element={<Dashboard />} />
        <Route path="/library/:id" element={<Library />} />
        <Route path="/search" element={<SearchResults />} />
        <Route path="/media/:id" element={<Media />} />
        <Route path="/preferences" element={<Preferences />} />
      </Route>
      <Route path="/play/:fileID" element={<VideoPlayer />} />
    </Route>
  </Routes>
);

const App = () => (
  <>
    <ThemeController />
    <FaviconController />
    <WS>
      <BrowserRouter>
        <AuthSessionController />
        <CrossTabAuthController />
        <ScrollToTop />
        <ApplicationRoutes />
      </BrowserRouter>
      <Notifications />
    </WS>
  </>
);

export default App;

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// OAuth 2.0 callback (oauth-callback.html): the app on this origin waits on the
// BroadcastChannel `requesttailor-oauth` (apps/web/src/auth.ts). The code is removed from the
// address bar and history right away; the window closes itself when it can.
(function () {
  "use strict";
  var url = window.location.href;
  try {
    window.history.replaceState(null, "", window.location.pathname);
  } catch (e) {
    // keep going
  }
  try {
    var channel = new BroadcastChannel("requesttailor-oauth");
    channel.postMessage({ url: url });
    channel.close();
  } catch (e) {
    var status = document.getElementById("status");
    if (status) status.textContent = "This browser cannot hand the sign-in back to RequestTailor.";
    return;
  }
  setTimeout(function () {
    window.close();
  }, 300);
})();

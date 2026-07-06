(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { HOME_SECTION_ID } = JWP.constants;

  const ensureHomeSection = (container) => {
    let section = document.getElementById(HOME_SECTION_ID);
    if (!section) {
      section = document.createElement('div');
      section.id = HOME_SECTION_ID;
      section.className = 'verticalSection verticalSection-extrabottompadding';
      container.prepend(section);
    }
    if (!state.rooms || state.rooms.length === 0) {
      if (section.parentNode) section.remove();
      return null;
    }
    let itemsContainer = section.querySelector('.itemsContainer');
    if (!itemsContainer) {
      section.innerHTML = `
        <div class="sectionTitleContainer sectionTitleContainer-cards padded-left padded-right">
          <h2 class="sectionTitle sectionTitle-cards">
            <span class="material-icons sectionTitleIcon" style="margin-right:8px;">groups</span>
            Watch Parties
          </h2>
        </div>
        <div class="emby-scroller" data-horizontal="true" data-centerfocus="true">
          <div is="emby-itemscontainer" class="itemsContainer scrollSlider focuscontainer-x padded-left padded-right"></div>
        </div>
      `;
      itemsContainer = section.querySelector('.itemsContainer');
    }
    return itemsContainer;
  };

  const reconcileCards = (itemsContainer, rooms) => {
    const existingCards = new Map();
    itemsContainer.querySelectorAll('.jwp-room-card').forEach(card => {
      existingCards.set(card.dataset.roomId, card);
    });
    const currentRoomIds = new Set(rooms.map(r => r.id));
    existingCards.forEach((card, roomId) => {
      if (!currentRoomIds.has(roomId)) {
        card.remove();
      }
    });
    rooms.forEach((room, index) => {
      const existing = existingCards.get(room.id);
      if (existing) {
        if (existing.dataset.count !== String(room.count)) {
          existing.dataset.count = room.count;
          const countEl = existing.querySelector('.innerCardFooter .cardText');
          if (countEl) {
            countEl.innerHTML = `<span class="material-icons" style="font-size:14px;vertical-align:middle;">groups</span> ${room.count} watching`;
          }
        }
      } else {
        itemsContainer.appendChild(ui.createRoomCard(room, index));
      }
    });
  };

  const renderHomeWatchParties = () => {
    if (!utils.isHomeView()) return;
    const container = document.querySelector('.homeSectionsContainer') || document.querySelector('#indexPage');
    if (!container) return;
    const itemsContainer = ensureHomeSection(container);
    if (!itemsContainer) return;
    reconcileCards(itemsContainer, state.rooms);
  };

  Object.assign(ui, { renderHomeWatchParties });
})();

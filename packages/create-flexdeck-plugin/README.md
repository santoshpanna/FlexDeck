# create-flexdeck-plugin

    npm create flexdeck-plugin lights "Desk lights"

A new FlexDeck plugin in `./lights`, copied from the template the FlexDeck repository tests, so it builds, passes its tests and packs from the start. Then:

    cd lights
    npm install
    npm test
    npm run verify      # ask FlexDeck itself whether it will work
    npm run package     # lights-0.1.0.fdpkg

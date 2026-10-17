'use strict';

const path = require('node:path');
module.exports = require('node-gyp-build')(path.join(__dirname, '..', '..'));
module.exports.nodeTypeInfo = require('../../src/node-types.json');

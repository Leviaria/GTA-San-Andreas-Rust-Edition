fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE Box taxi job: take a dispatch, drive to it, get paid. The server checks every step.'
version '0.1.0'

dependency 'sarebox'
shared_script 'config.lua'
server_script 'server.lua'
client_script 'client.lua'
